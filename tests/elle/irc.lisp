## tests/elle/irc.lisp — IRC module tests
##
## Pure parsing and formatting tests. No network required.

(def irc ((import-file "lib/irc.lisp")))

## ── Verify exports ──────────────────────────────────────────────────

(assert (fn? irc:connect) "export: connect is a function")
(assert (fn? irc:parse-message) "export: parse-message is a function")
(assert (fn? irc:format-message) "export: format-message is a function")
(assert (fn? irc:parse-tags) "export: parse-tags is a function")
(assert (fn? irc:parse-source) "export: parse-source is a function")
(assert (fn? irc:parse-ctcp) "export: parse-ctcp is a function")
(assert (fn? irc:test) "export: test is a function")

## ── Run internal tests ──────────────────────────────────────────────

(assert (irc:test) "internal tests pass")

## ── Additional edge cases ───────────────────────────────────────────

# Parse a CAP LS line
(let [[msg (irc:parse-message ":server CAP * LS :multi-prefix sasl server-time")]]
  (assert (= msg:command "CAP") "CAP LS: command")
  (assert (= (get msg:params 0) "*") "CAP LS: target")
  (assert (= (get msg:params 1) "LS") "CAP LS: subcommand")
  (assert (= (get msg:params 2) "multi-prefix sasl server-time") "CAP LS: caps"))

# Parse 001 welcome
(let [[msg (irc:parse-message ":irc.libera.chat 001 ellebot :Welcome to Libera.Chat")]]
  (assert (= msg:command "001") "001: command")
  (assert (= msg:source:server "irc.libera.chat") "001: server source")
  (assert (= (get msg:params 0) "ellebot") "001: nick")
  (assert (string/starts-with? (get msg:params 1) "Welcome") "001: welcome text"))

# Parse NICK change
(let [[msg (irc:parse-message ":old!user@host NICK :new")]]
  (assert (= msg:command "NICK") "NICK: command")
  (assert (= msg:source:nick "old") "NICK: old nick")
  (assert (= (get msg:params 0) "new") "NICK: new nick"))

# Parse numeric with many params
(let [[msg (irc:parse-message ":server 004 bot irc.server ircd-2.0 iowRs biklmnopst")]]
  (assert (= msg:command "004") "004: command")
  (assert (= (get msg:params 0) "bot") "004: nick")
  (assert (= (get msg:params 1) "irc.server") "004: server name")
  (assert (= (get msg:params 2) "ircd-2.0") "004: version")
  (assert (= (get msg:params 3) "iowRs") "004: user modes")
  (assert (= (get msg:params 4) "biklmnopst") "004: channel modes"))

# Tags with escaped values
(let [[msg (irc:parse-message "@time=2024-01-15T16:40:51.620Z;msgid=abc123 :nick!u@h PRIVMSG #ch :hello")]]
  (assert (= msg:tags:time "2024-01-15T16:40:51.620Z") "tagged msg: time")
  (assert (= msg:tags:msgid "abc123") "tagged msg: msgid")
  (assert (= (get msg:params 1) "hello") "tagged msg: text"))

# Format roundtrip with tags
(let* [[msg {:tags {:time "2024-01-01"} :source {:nick "n" :user "u" :host "h"}
             :command "PRIVMSG" :params ["#ch" "hi there"]}]
       [line (irc:format-message msg)]
       [reparsed (irc:parse-message line)]]
  (assert (= reparsed:command "PRIVMSG") "tag roundtrip: command")
  (assert (= reparsed:tags:time "2024-01-01") "tag roundtrip: time tag")
  (assert (= reparsed:source:nick "n") "tag roundtrip: nick")
  (assert (= (get reparsed:params 1) "hi there") "tag roundtrip: text"))

(println "irc: all tests passed")
