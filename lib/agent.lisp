## lib/agent.lisp — LLM agent subprocess abstraction
##
## Loaded via: (def agent ((import-file "lib/agent.lisp")))
## Usage:
##   (def handle (agent:make-handle {:backend :claude :model "sonnet"}))
##   (stream/for-each
##     (fn [c] (print c:text))
##     (agent:send handle "review lib/http.lisp"))
##
## One subprocess per turn. Session continuation via --resume (Claude)
## or --session + --continue (OpenCode). The handle is mutable and
## tracks the session ID across sends.
##
## Chunk types: :text, :tool-use, :tool-input, :stderr, :result


# ============================================================================
# Flag table — config key → CLI flags per backend
# ============================================================================

# Each entry: {:key config-key :claude flag :opencode flag :type how-to-emit}
# :type is one of:
#   :val    — emit [flag value]
#   :bool   — emit [flag] when truthy
#   :each   — emit [flag item] for each item in array
#   :coerce — emit [flag (string value)]

(def flag-table
  [{:key :model          :claude "--model"                       :opencode "-m"        :type :val}
   {:key :system-prompt  :claude "--system-prompt"               :opencode "--prompt"   :type :val}
   {:key :dir            :claude "--add-dir"                     :opencode "--dir"      :type :val}
   {:key :effort         :claude "--effort"                      :opencode "--variant"  :type :coerce}
   {:key :max-budget     :claude "--max-budget-usd"              :opencode nil          :type :coerce}
   {:key :skip-permissions :claude "--dangerously-skip-permissions" :opencode nil       :type :bool}
   {:key :allowed-tools  :claude "--allowedTools"                :opencode nil          :type :each}
   {:key :denied-tools   :claude "--disallowedTools"             :opencode nil          :type :each}])


# ============================================================================
# build-args — config + prompt + session-id → CLI arg array
# ============================================================================

(defn build-args [config prompt session-id]
  "Build CLI argument array from config, prompt, and optional session-id."
  (let* [[backend (get config :backend)]
         [args    @[]]]

    # Binary and output format
    (case backend
      :claude   (begin (push args "claude")
                       (push args "--output-format") (push args "stream-json")
                       (push args "--verbose"))
      :opencode (begin (push args "opencode")
                       (push args "--format") (push args "json"))
      (error {:error :agent-error
              :message (string "unknown backend: " backend)}))

    # Session resumption
    (when (not (nil? session-id))
      (case backend
        :claude   (begin (push args "--resume") (push args session-id))
        :opencode (begin (push args "--session") (push args session-id)
                         (push args "--continue"))))

    # Table-driven flags
    (each entry in flag-table
      (let* [[val  (get config entry:key nil)]
             [flag (get entry backend nil)]]
        (when (and (not (nil? val)) (not (nil? flag)))
          (case entry:type
            :val    (begin (push args flag) (push args val))
            :coerce (begin (push args flag) (push args (string val)))
            :bool   (push args flag)
            :each   (each item in val
                      (push args flag) (push args item))))))

    # Passthrough opts
    (when (not (nil? (get config :opts nil)))
      (each opt in (get config :opts)
        (push args opt)))

    # Prompt last
    (push args "-p")
    (push args prompt)

    (freeze args)))


# ============================================================================
# normalize-claude — JSON line → chunk or nil
# ============================================================================

(defn normalize-claude [line]
  "Parse a Claude stream-json line into a chunk, or nil to skip."
  (let [[obj (json/parse line :keys :keyword)]]
    (when (not (nil? obj))
      (case obj:type
        "content_block_delta"
          (let [[delta (get obj :delta {})]]
            (case delta:type
              "text_delta"       {:text delta:text :type :text}
              "input_json_delta" {:text (get delta :partial_json "")
                                  :type :tool-input}
              nil))
        "content_block_start"
          (let [[cb (get obj :content_block {})]]
            (when (= cb:type "tool_use")
              {:type :tool-use :name cb:name :id cb:id}))
        "result"
          (let [[usage (get obj :usage {})]]
            {:text       (get obj :result "")
             :type       :result
             :cost       (get obj :total_cost_usd 0)
             :session-id obj:session_id
             :tokens     {:input  (get usage :input_tokens 0)
                          :output (get usage :output_tokens 0)}})
        nil))))


# ============================================================================
# normalize-opencode — JSON line → chunk or nil
# ============================================================================

(defn normalize-opencode [line]
  "Parse an OpenCode JSON line into a chunk, or nil to skip."
  (let [[obj (json/parse line :keys :keyword)]]
    (when (not (nil? obj))
      (let [[part (get obj :part {})]]
        (case obj:type
          "text"        {:text part:text :type :text}
          "step_finish" {:type   :result
                         :cost   (get part :cost 0)
                         :tokens (get part :tokens nil)
                         :text   ""}
          nil)))))


# ============================================================================
# make-handle — create a mutable agent handle
# ============================================================================

(defn make-handle [config]
  "Create a mutable agent handle from config."
  @{:config config :session-id nil :total-cost 0 :proc nil})


# ============================================================================
# kill — terminate the current subprocess
# ============================================================================

(defn kill [handle]
  "Kill the current subprocess, if any."
  (when (not (nil? handle:proc))
    (protect (subprocess/kill handle:proc))
    (protect (subprocess/wait handle:proc))
    (put handle :proc nil)))


# ============================================================================
# send — spawn subprocess, return stream of chunks
# ============================================================================

(defn send [handle prompt]
  "Send a prompt to the agent. Returns a stream of chunks.
   If config has :command [program args...], uses that instead of build-args."
  (let* [[config     handle:config]
         [session-id handle:session-id]
         [backend    config:backend]
         [cmd        (get config :command nil)]
         [args       (if (nil? cmd)
                       (build-args config prompt session-id)
                       cmd)]
         [program    (first args)]
         [rest-args  (slice args 1)]
         [normalize  (case backend
                       :claude   normalize-claude
                       :opencode normalize-opencode)]]
    (coro/new (fn []
      # TODO: swap :null for :nil when subprocess is updated
      (let* [[proc   (subprocess/exec program rest-args)]
             [stdout proc:stdout]
             [stderr proc:stderr]]
        (put handle :proc proc)

        # Drain stderr in parallel — collect lines
        (var stderr-lines @[])
        (var lc nil)
        (let [[stderr-fiber (ev/spawn (fn []
                (stream/for-each
                  (fn [line] (push stderr-lines line))
                  (port/lines stderr))))]]

          # Drain stdout line by line
          (while true
            # Flush accumulated stderr as chunks
            (each sl in stderr-lines
              (yield {:type :stderr :text sl}))
            (while (> (length stderr-lines) 0) (pop stderr-lines))

            # Read next stdout line
            (let [[line (port/read-line stdout)]]
              (when (nil? line) (break))
              (when (> (length line) 0)
                (let [[[ok? parsed] (protect (normalize line))]]
                  (if (not ok?)
                    (yield {:type :stderr :text (string "parse error: " parsed)})
                    (when (not (nil? parsed))
                      (assign lc parsed)
                      (yield parsed)))))))

          # Join stderr fiber, flush remaining
          (protect (ev/join stderr-fiber))
          (each sl in stderr-lines
            (yield {:type :stderr :text sl})))

        (let [[exit (subprocess/wait proc)]]
          (put handle :proc nil)
          # Update session-id and accumulate cost from result chunk
          (when (and (not (nil? lc))
                     (= lc:type :result))
            (when (not (nil? (get lc :session-id nil)))
              (put handle :session-id lc:session-id))
            (when (not (nil? (get lc :cost nil)))
              (put handle :total-cost
                   (+ handle:total-cost lc:cost))))
          # Signal error on nonzero exit with no result chunk
          (when (and (not (= exit 0))
                     (or (nil? lc) (not (= lc:type :result))))
            (error {:error :agent-error
                    :message (string program " exited with code " exit)}))))))))


# ============================================================================
# send-collect — convenience: drain stream, return final result
# ============================================================================

(defn send-collect [handle prompt]
  "Send a prompt and collect the full response. Returns result struct
   with :text containing the concatenated text."
  (var result nil)
  (var text @"")
  (stream/for-each
    (fn [chunk]
      (case chunk:type
        :text   (push text chunk:text)
        :result (assign result chunk)
        nil))
    (send handle prompt))
  (if (nil? result)
    {:type :result :text (freeze text) :cost 0}
    (merge result {:text (freeze text)})))


# ============================================================================
# Exports
# ============================================================================

(fn []
  {:make-handle  make-handle
   :send         send
   :send-collect send-collect
   :kill         kill
   :build-args   build-args})
