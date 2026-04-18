(elle/epoch 7)
## lib/irc.lisp -- IRCv3 client for Elle
##
## Coroutine-based IRC client with IRCv3 capability negotiation and SASL.
## The connection struct carries a read stream (coroutine yielding parsed
## messages with auto-PONG) and a send function.
##
## Usage:
##   (def tls ((import "std/tls") (import "plugin/tls")))
##   (def irc ((import "std/irc") :tls tls))
##   (def conn (irc:connect "irc.libera.chat" 6697 :nick "ellebot"))
##   (conn:send "JOIN" "#test")
##   (each msg in conn:messages
##     (when (= msg:command "PRIVMSG")
##       (let [[[target text] msg:params]]
##         (println msg:source:nick ": " text)
##         (conn:send "PRIVMSG" target "I heard you!"))))
##   (conn:close)
##
## Connection struct:
##   {:messages <coroutine>  -- yields parsed messages (auto-PONG)
##    :send     <function>   -- (conn:send "COMMAND" "param1" ...)
##    :close    <function>   -- sends QUIT, closes transport
##    :nick     "nick"       -- resolved nick after registration
##    :caps     |...|        -- negotiated IRCv3 capabilities
##    :server   "hostname"   -- server name from RPL_MYINFO
##    :isupport {...}}       -- parsed ISUPPORT (005) parameters
##
## Message struct:
##   {:tags    {:time "..." :msgid "..."}  -- or nil
##    :source  {:nick "n" :user "u" :host "h"}  -- or {:server "name"} or nil
##    :command "PRIVMSG"                   -- always uppercase
##    :params  ["#channel" "Hello world"]} -- array of strings
##
## Spawned-reader/queue pattern (for background PING handling):
##   (def sync ((import "std/sync")))
##   (def q (sync:make-queue 256))
##   (ev/spawn (fn [] (each msg in conn:messages (q:put msg))))

(fn [&named tls]

  (def b64 ((import "std/base64")))

  ## ── Constants ─────────────────────────────────────────────────────

  (def DEFAULT-PORT-PLAIN 6667)
  (def DEFAULT-PORT-TLS   6697)
  (def SOH (string (bytes 1)))

  (def DESIRED-CAPS
    ["multi-prefix" "server-time" "echo-message" "account-notify"
     "away-notify" "extended-join" "chghost" "userhost-in-names"
     "message-tags" "batch" "labeled-response"])

  ## ── Tag escaping ──────────────────────────────────────────────────

  (defn escape-tag-value [s]
    "Escape an IRCv3 tag value per the message-tags spec.
     \\ -> \\\\ | ; -> \\: | space -> \\s | CR -> \\r | LF -> \\n"
    (let* [s (string/replace s "\\" "\\\\")
           s (string/replace s ";" "\\:")
           s (string/replace s " " "\\s")
           s (string/replace s "\r" "\\r")
           s (string/replace s "\n" "\\n")]
      s))

  (defn unescape-tag-value [s]
    "Unescape an IRCv3 tag value. Character-by-character scan to avoid
     multi-pass replacement ambiguity."
    (var result @"")
    (var i 0)
    (while (< i (length s))
      (if (and (= (get s i) "\\") (< (inc i) (length s)))
        (let [next (get s (inc i))]
          (append result
            (match next
              [":" ";"]
              ["s" " "]
              ["\\" "\\"]
              ["r" "\r"]
              ["n" "\n"]
              [_ next]))
          (assign i (+ i 2)))
        (begin
          (append result (get s i))
          (assign i (inc i)))))
    (freeze result))

  ## ── Message parsing ───────────────────────────────────────────────

  (defn parse-tags [raw]
    "Parse IRCv3 tags string (without leading @) into a struct.
     'time=2024-01-01;msgid=abc;account' -> {:time '...' :msgid 'abc' :account true}"
    (def result @{})
    (each part in (string/split raw ";")
      (when (> (length part) 0)
        (let [eq (string/find part "=")]
          (if eq
            (put result (keyword (slice part 0 eq))
                        (unescape-tag-value (slice part (inc eq))))
            (put result (keyword part) true)))))
    (freeze result))

  (defn parse-source [raw]
    "Parse nick!user@host or servername into a struct.
     'nick!user@host.com' -> {:nick 'nick' :user 'user' :host 'host.com'}
     'irc.server.net'     -> {:server 'irc.server.net'}
     'justnick'           -> {:nick 'justnick' :user nil :host nil}"
    (let [bang (string/find raw "!")]
      (if bang
        (let* [nick (slice raw 0 bang)
               rest (slice raw (inc bang))
               at (string/find rest "@")]
          (if at
            {:nick nick :user (slice rest 0 at) :host (slice rest (inc at))}
            {:nick nick :user rest :host nil}))
        (if (string/contains? raw ".")
          {:server raw}
          {:nick raw :user nil :host nil}))))

  (defn parse-params [s]
    "Parse IRC parameter string into an array of strings.
     Middle params are space-delimited. A param starting with : is trailing
     (contains the rest of the line as a single param)."
    (def params @[])
    (var rest s)
    (forever
      (while (and (> (length rest) 0) (= (get rest 0) " "))
        (assign rest (slice rest 1)))
      (when (= (length rest) 0) (break))
      (if (= (get rest 0) ":")
        (begin (push params (slice rest 1)) (break))
        (let [sp (string/find rest " ")]
          (if sp
            (begin (push params (slice rest 0 sp))
                   (assign rest (slice rest (inc sp))))
            (begin (push params rest) (break))))))
    (freeze params))

  (defn parse-message [line]
    "Parse an IRC protocol line into {:tags :source :command :params}.
     Tags and source are nil if absent. Command is uppercased."
    (var rest line)
    (var tags nil)
    (var source nil)

    # Tags: @key=val;key2 ...
    (when (and (> (length rest) 0) (= (get rest 0) "@"))
      (let [sp (string/find rest " ")]
        (when sp
          (assign tags (parse-tags (slice rest 1 sp)))
          (assign rest (slice rest (inc sp))))))

    # Skip spaces
    (while (and (> (length rest) 0) (= (get rest 0) " "))
      (assign rest (slice rest 1)))

    # Source: :nick!user@host ...
    (when (and (> (length rest) 0) (= (get rest 0) ":"))
      (let [sp (string/find rest " ")]
        (when sp
          (assign source (parse-source (slice rest 1 sp)))
          (assign rest (slice rest (inc sp))))))

    # Skip spaces
    (while (and (> (length rest) 0) (= (get rest 0) " "))
      (assign rest (slice rest 1)))

    # Command and params
    (let* [sp (string/find rest " ")
           command (if sp (slice rest 0 sp) rest)
           param-str (if sp (slice rest (inc sp)) "")]
      {:tags tags
       :source source
       :command (string/upcase command)
       :params (if (= (length param-str) 0) [] (parse-params param-str))}))

  ## ── Message formatting ────────────────────────────────────────────

  (defn build-line [command params]
    "Build an IRC protocol line from command and params array.
     Last param is auto-prefixed with : if it contains spaces, starts
     with :, or is empty."
    (if (= (length params) 0)
      command
      (let* [n (length params)
             last-param (get params (dec n))
             needs-colon (or (string/contains? last-param " ")
                             (string/starts-with? last-param ":")
                             (= last-param ""))]
        (var parts @[command])
        (each i in (range (dec n))
          (push parts (get params i)))
        (push parts (if needs-colon (string ":" last-param) last-param))
        (string/join (freeze parts) " "))))

  (defn format-tags [tags]
    "Format a tags struct to key1=val1;key2 string (without leading @)."
    (def parts @[])
    (each [k v] in (pairs tags)
      (push parts (if (= v true)
                    (string k)
                    (string k "=" (escape-tag-value (string v))))))
    (string/join (freeze parts) ";"))

  (defn format-source [source]
    "Format a source struct to nick!user@host or servername string."
    (if (get source :server)
      source:server
      (if (and source:user source:host)
        (string source:nick "!" source:user "@" source:host)
        (if source:user
          (string source:nick "!" source:user)
          source:nick))))

  (defn format-message [msg]
    "Format a parsed message struct back to an IRC protocol line.
     Inverse of parse-message (modulo command case and optional : on trailing)."
    (var parts @[])
    (when msg:tags   (push parts (string "@" (format-tags msg:tags))))
    (when msg:source (push parts (string ":" (format-source msg:source))))
    (push parts msg:command)
    (when (> (length msg:params) 0)
      (let* [n (length msg:params)
             last-param (get msg:params (dec n))
             needs-colon (or (string/contains? last-param " ")
                             (string/starts-with? last-param ":")
                             (= last-param ""))]
        (each i in (range (dec n))
          (push parts (get msg:params i)))
        (push parts (if needs-colon (string ":" last-param) last-param))))
    (string/join (freeze parts) " "))

  ## ── CTCP ──────────────────────────────────────────────────────────

  (defn parse-ctcp [text]
    "Parse CTCP from message text. Returns {:command :text} or nil.
     CTCP messages are delimited by SOH (0x01) characters."
    (when (and (string/starts-with? text SOH)
               (string/ends-with? text SOH)
               (> (length text) 1))
      (let* [inner (slice text 1 (dec (length text)))
             sp (string/find inner " ")]
        (if sp
          {:command (slice inner 0 sp) :text (slice inner (inc sp))}
          {:command inner :text nil}))))

  ## ── SASL ──────────────────────────────────────────────────────────

  (defn sasl-plain-payload [authcid password]
    "Build SASL PLAIN payload: base64(NUL + authcid + NUL + password)."
    (def buf (@bytes))
    (append buf (bytes 0))
    (append buf (bytes authcid))
    (append buf (bytes 0))
    (append buf (bytes password))
    (b64:encode (freeze buf)))

  ## ── ISUPPORT ──────────────────────────────────────────────────────

  (defn parse-isupport [tokens]
    "Parse 005 ISUPPORT tokens into a struct.
     'CHANTYPES=#&' -> {:chantypes '#&'}. Tokens without = are ignored."
    (def result @{})
    (each token in tokens
      (let [eq (string/find token "=")]
        (when eq
          (put result
            (keyword (string/downcase (slice token 0 eq)))
            (slice token (inc eq))))))
    (freeze result))

  ## ── Transport ─────────────────────────────────────────────────────

  (defn strip-crlf [s]
    "Strip trailing CRLF, LF, or CR from a line.
     CRLF is a single grapheme in Elle, so all cases strip one grapheme."
    (if (or (string/ends-with? s "\r\n")
            (string/ends-with? s "\n")
            (string/ends-with? s "\r"))
      (slice s 0 (dec (length s)))
      s))

  (defn make-transport [host port-num]
    "Create a transport struct {:read-line :write :close} for host:port.
     Uses TLS if the tls module was provided to the constructor."
    (if tls
      (let [conn (tls:connect host port-num)]
        {:read-line (fn [] (let [line (tls:read-line conn)]
                             (when line (strip-crlf line))))
         :write     (fn [data] (tls:write conn (string data "\r\n")))
         :close     (fn [] (tls:close conn))})
      (let* [ip (first (sys/resolve host))
             port (tcp/connect ip port-num)]
        {:read-line (fn [] (port/read-line port))
         :write     (fn [data] (port/write port (string data "\r\n"))
                               (port/flush port))
         :close     (fn [] (port/close port))})))

  ## ── Registration ──────────────────────────────────────────────────

  (defn register [transport nick username realname &named sasl]
    "Perform IRC connection registration with IRCv3 CAP negotiation.
     sasl: [authcid password] or nil. Returns registration result struct."
    (defn send [line] ((get transport :write) line))
    (defn recv []
      (let [line ((get transport :read-line))]
        (when line (parse-message line))))

    (send (build-line "CAP" ["LS" "302"]))
    (send (build-line "NICK" [nick]))
    (send (build-line "USER" [username "0" "*" realname]))

    (var server-caps @[])
    (var negotiated-caps ||)
    (var current-nick nick)
    (var nick-retries 3)
    (var server-name nil)
    (var isupport-map @{})
    (var sasl-in-progress false)

    (defn handle-cap-ls [msg]
      (let* [has-more (and (>= (length msg:params) 4)
                            (= (get msg:params 2) "*"))
             cap-str (if has-more (get msg:params 3) (get msg:params 2))]
        (each cap in (string/split cap-str " ")
          (when (> (length cap) 0)
            (let [eq (string/find cap "=")]
              (push server-caps (if eq (slice cap 0 eq) cap)))))
        (unless has-more
          (let* [offered (apply set (freeze server-caps))
                 desired (if sasl ["sasl" ;DESIRED-CAPS] DESIRED-CAPS)
                 to-req @[]]
            (each cap in desired
              (when (contains? offered cap) (push to-req cap)))
            (if (> (length to-req) 0)
              (send (build-line "CAP"
                      ["REQ" (string/join (freeze to-req) " ")]))
              (send (build-line "CAP" ["END"])))))))

    (defn handle-cap-ack [msg]
      (let [acked (string/split
                     (get msg:params (dec (length msg:params))) " ")]
        (assign negotiated-caps
          (apply set (filter (fn [s] (> (length s) 0)) acked))))
      (if (and sasl (contains? negotiated-caps "sasl"))
        (begin (send "AUTHENTICATE PLAIN")
               (assign sasl-in-progress true))
        (send (build-line "CAP" ["END"]))))

    (defn handle-cap [msg]
      (when (>= (length msg:params) 3)
        (match (get msg:params 1)
          ["LS"  (handle-cap-ls msg)]
          ["ACK" (handle-cap-ack msg)]
          ["NAK" (send (build-line "CAP" ["END"]))]
          [_ nil])))

    (defn handle-auth [msg]
      (when (and sasl-in-progress (= (get msg:params 0) "+"))
        (let [[authcid password] sasl]
          (send (build-line "AUTHENTICATE"
                  [(sasl-plain-payload authcid password)])))))

    (defn handle-nick-collision []
      (when (zero? nick-retries)
        (error {:error :irc-error :reason :nick-collision :nick current-nick :message "nick collision: retries exhausted"}))
      (assign current-nick (string current-nick "_"))
      (assign nick-retries (dec nick-retries))
      (send (build-line "NICK" [current-nick])))

    (defn handle-isupport [msg]
      (when (> (length msg:params) 2)
        (let [tokens (slice msg:params 1 (dec (length msg:params)))]
          (each [k v] in (pairs (parse-isupport [;tokens]))
            (put isupport-map k v)))))

    (forever
      (let [msg (recv)]
        (when (nil? msg)
          (error {:error :irc-error :reason :connection-closed :phase :registration :message "connection closed during registration"}))
        (match msg:command
          ["CAP"          (handle-cap msg)]
          ["AUTHENTICATE" (handle-auth msg)]
          ["903"          (assign sasl-in-progress false)
                          (send (build-line "CAP" ["END"]))]
          ["904"          (error {:error :irc-error :reason :sasl-failed
                                  :message "SASL authentication failed"})]
          ["433"          (handle-nick-collision)]
          ["004"          (when (>= (length msg:params) 2)
                            (assign server-name (get msg:params 1)))]
          ["005"          (handle-isupport msg)]
          ["PING"         (send (build-line "PONG"
                                  [(or (get msg:params 0) "")]))]
          ["001"          (break {:nick current-nick
                                  :caps negotiated-caps
                                  :server (or server-name "unknown")
                                  :isupport (freeze isupport-map)})]
          [_ nil]))))

  ## ── Connect ───────────────────────────────────────────────────────

  (defn irc/connect [host port &named nick username realname sasl]
    "Connect to an IRC server and complete registration.
     Returns a connection struct with :messages, :send, :close, :nick,
     :caps, :server, :isupport.

     host:     server hostname
     port:     server port (6697 for TLS, 6667 for plain)
     :nick     nickname (required)
     :username ident username (default: nick)
     :realname real name (default: nick)
     :sasl     [authcid password] for SASL PLAIN auth"
    (when (nil? nick)
      (error {:error :irc-error :reason :missing-param :param :nick :message "irc:connect requires :nick"}))
    (default username nick)
    (default realname nick)

    (let [transport (make-transport host port)]
      (let [[ok? result] (protect
                            (register transport nick username realname
                                      :sasl sasl))]
        (unless ok?
          (protect ((get transport :close)))
          (error result))

        (let* [write-fn (get transport :write)
               read-fn  (get transport :read-line)
               close-fn (get transport :close)
               messages
                (coro/new (fn []
                  (forever
                    (let [line (read-fn)]
                      (when (nil? line) (break))
                      (let [msg (parse-message line)]
                        (if (= msg:command "PING")
                          (write-fn (build-line "PONG"
                                      [(or (get msg:params 0) "")]))
                          (yield msg)))))))]

          {:messages messages
           :send     (fn [command & params]
                       (write-fn (build-line command [;params])))
           :close    (fn [& args]
                       (let [message (or (get args 0) "Leaving")]
                         (protect (write-fn (build-line "QUIT" [message])))
                         (close-fn)))
           :nick     result:nick
           :caps     result:caps
           :server   result:server
           :isupport result:isupport}))))

  ## ── Internal tests ────────────────────────────────────────────────

  (defn run-internal-tests []
    "Pure tests on parsing, formatting, tag escaping, SASL. No network."

    ## ── Tag escaping ──

    (assert (= (escape-tag-value "hello world") "hello\\sworld")
      "escape: space")
    (assert (= (escape-tag-value "a;b") "a\\:b")
      "escape: semicolon")
    (assert (= (escape-tag-value "a\\b") "a\\\\b")
      "escape: backslash")
    (assert (= (escape-tag-value "plain") "plain")
      "escape: no special chars")

    (assert (= (unescape-tag-value "hello\\sworld") "hello world")
      "unescape: space")
    (assert (= (unescape-tag-value "a\\:b") "a;b")
      "unescape: semicolon")
    (assert (= (unescape-tag-value "a\\\\b") "a\\b")
      "unescape: backslash")
    (assert (= (unescape-tag-value "plain") "plain")
      "unescape: no escapes")

    # Roundtrip
    (assert (= (unescape-tag-value (escape-tag-value "a;b c\\d\r\n"))
               "a;b c\\d\r\n")
      "tag escape roundtrip")

    ## ── Tag parsing ──

    (let [tags (parse-tags "time=2024-01-01T00:00:00Z;msgid=abc123")]
      (assert (= tags:time "2024-01-01T00:00:00Z") "parse-tags: time")
      (assert (= tags:msgid "abc123") "parse-tags: msgid"))

    (let [tags (parse-tags "account")]
      (assert (= tags:account true) "parse-tags: valueless"))

    (let [tags (parse-tags "a=hello\\sworld;b=x\\:y")]
      (assert (= tags:a "hello world") "parse-tags: escaped space")
      (assert (= tags:b "x;y") "parse-tags: escaped semicolon"))

    ## ── Source parsing ──

    (let [src (parse-source "nick!user@host.com")]
      (assert (= src:nick "nick") "parse-source: nick")
      (assert (= src:user "user") "parse-source: user")
      (assert (= src:host "host.com") "parse-source: host"))

    (let [src (parse-source "irc.server.net")]
      (assert (= src:server "irc.server.net") "parse-source: server"))

    (let [src (parse-source "justnick")]
      (assert (= src:nick "justnick") "parse-source: nick only")
      (assert (nil? src:user) "parse-source: nick only no user")
      (assert (nil? src:host) "parse-source: nick only no host"))

    ## ── Message parsing ──

    (let [msg (parse-message "PING :token123")]
      (assert (= msg:command "PING") "parse PING command")
      (assert (= (get msg:params 0) "token123") "parse PING token")
      (assert (nil? msg:source) "parse PING no source")
      (assert (nil? msg:tags) "parse PING no tags"))

    (let [msg (parse-message ":nick!user@host PRIVMSG #channel :Hello world")]
      (assert (= msg:command "PRIVMSG") "parse PRIVMSG command")
      (assert (= msg:source:nick "nick") "parse PRIVMSG nick")
      (assert (= msg:source:user "user") "parse PRIVMSG user")
      (assert (= msg:source:host "host") "parse PRIVMSG host")
      (assert (= (get msg:params 0) "#channel") "parse PRIVMSG target")
      (assert (= (get msg:params 1) "Hello world") "parse PRIVMSG text"))

    (let [msg (parse-message "@time=2024-01-01T00:00:00Z :nick!u@h PRIVMSG #ch :hi")]
      (assert (= msg:tags:time "2024-01-01T00:00:00Z") "parse tagged: time")
      (assert (= msg:command "PRIVMSG") "parse tagged: command")
      (assert (= (get msg:params 1) "hi") "parse tagged: text"))

    # Multiple middle params + trailing
    (let [msg (parse-message ":server 005 bot CHANTYPES=#& PREFIX=(ov)@+ :are supported")]
      (assert (= msg:command "005") "parse 005 command")
      (assert (= (get msg:params 0) "bot") "parse 005 target")
      (assert (= (get msg:params 1) "CHANTYPES=#&") "parse 005 chantypes")
      (assert (= (get msg:params 2) "PREFIX=(ov)@+") "parse 005 prefix")
      (assert (= (get msg:params 3) "are supported") "parse 005 trailing"))

    # No params
    (let [msg (parse-message "QUIT")]
      (assert (= msg:command "QUIT") "parse no-param command")
      (assert (= (length msg:params) 0) "parse no-param empty"))

    # Command only with trailing
    (let [msg (parse-message "ERROR :Closing Link")]
      (assert (= msg:command "ERROR") "parse ERROR command")
      (assert (= (get msg:params 0) "Closing Link") "parse ERROR text"))

    ## ── Message formatting ──

    (assert (= (build-line "JOIN" ["#test"]) "JOIN #test")
      "format JOIN")
    (assert (= (build-line "PRIVMSG" ["#ch" "Hello world"])
               "PRIVMSG #ch :Hello world")
      "format PRIVMSG with trailing")
    (assert (= (build-line "NICK" ["bot"]) "NICK bot")
      "format NICK")
    (assert (= (build-line "QUIT" ["Bye bye"]) "QUIT :Bye bye")
      "format QUIT with spaces")
    (assert (= (build-line "PING" ["token"]) "PING token")
      "format PING")

    # Format/parse structural roundtrip
    (let* [original ":nick!user@host PRIVMSG #channel :Hello world"
           parsed (parse-message original)
           formatted (format-message parsed)
           reparsed (parse-message formatted)]
      (assert (= reparsed:command parsed:command) "roundtrip: command")
      (assert (= (get reparsed:params 0) (get parsed:params 0)) "roundtrip: target")
      (assert (= (get reparsed:params 1) (get parsed:params 1)) "roundtrip: text")
      (assert (= reparsed:source:nick parsed:source:nick) "roundtrip: nick"))

    ## ── CTCP ──

    (let [ctcp (parse-ctcp (string SOH "VERSION" SOH))]
      (assert (= ctcp:command "VERSION") "ctcp VERSION command")
      (assert (nil? ctcp:text) "ctcp VERSION no text"))

    (let [ctcp (parse-ctcp (string SOH "PING 12345" SOH))]
      (assert (= ctcp:command "PING") "ctcp PING command")
      (assert (= ctcp:text "12345") "ctcp PING text"))

    (let [ctcp (parse-ctcp (string SOH "ACTION waves" SOH))]
      (assert (= ctcp:command "ACTION") "ctcp ACTION command")
      (assert (= ctcp:text "waves") "ctcp ACTION text"))

    (assert (nil? (parse-ctcp "not ctcp")) "ctcp: plain text")
    (assert (nil? (parse-ctcp "")) "ctcp: empty string")

    ## ── SASL ──

    (let* [payload (sasl-plain-payload "jilles" "sesame")
           decoded (b64:decode payload)]
      (assert (string? payload) "sasl payload is string")
      (assert (> (length payload) 0) "sasl payload nonempty")
      (assert (= (get decoded 0) 0) "sasl: leading NUL")
      (assert (= (string (slice decoded 1 7)) "jilles") "sasl: authcid")
      (assert (= (get decoded 7) 0) "sasl: middle NUL")
      (assert (= (string (slice decoded 8)) "sesame") "sasl: password"))

    ## ── ISUPPORT ──

    (let [params (parse-isupport ["CHANTYPES=#&" "PREFIX=(ov)@+" "NETWORK=Libera"])]
      (assert (= params:chantypes "#&") "isupport: CHANTYPES")
      (assert (= params:prefix "(ov)@+") "isupport: PREFIX")
      (assert (= params:network "Libera") "isupport: NETWORK"))

    (let [params (parse-isupport ["CHANTYPES=#" "SAFELIST" "NETWORK=Test"])]
      (assert (= params:chantypes "#") "isupport: with value")
      (assert (nil? (get params :safelist)) "isupport: no-value skipped")
      (assert (= params:network "Test") "isupport: network"))

    ## ── strip-crlf ──

    (assert (= (strip-crlf "hello\r\n") "hello") "strip-crlf: CRLF")
    (assert (= (strip-crlf "hello\n") "hello") "strip-crlf: LF")
    (assert (= (strip-crlf "hello\r") "hello") "strip-crlf: CR")
    (assert (= (strip-crlf "hello") "hello") "strip-crlf: none")

    true)

  ## ── Exports ───────────────────────────────────────────────────────

  {:connect        irc/connect
   :parse-message  parse-message
   :format-message format-message
   :parse-tags     parse-tags
   :parse-source   parse-source
   :parse-ctcp     parse-ctcp
   :test           run-internal-tests})
