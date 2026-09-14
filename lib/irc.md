# irc

<!-- audited: 2026-09-14 -->

IRCv3 client with capability negotiation, SASL PLAIN and message tags, over plain TCP or TLS.

The export struct at the bottom of [irc.lisp](irc.lisp) lists what the
module offers, and `(doc name)` carries each function's arguments. This
file holds the two structs a caller reads.

## Loading

```lisp
(def irc ((import "std/irc")))                             # plain TCP
(def irc ((import "std/irc") :tls ((import "std/tls")
                                    (import "plugin/tls"))))
```

`irc:connect` sends `CAP LS 302`, `NICK` and `USER`, negotiates
capabilities, authenticates when you passed `:sasl`, sends `CAP END`,
and returns once the server answers 001.

## Connection and message

```lisp
{:messages <fiber>     # |:yield| fiber of parsed messages, PINGs answered
 :send     <function>  # (conn:send "PRIVMSG" "#chan" "hello")
 :close    <function>  # QUIT, then close the transport
 :nick     "nick"      # what registration settled on
 :caps     |"multi-prefix" "server-time"|
 :server   "irc.libera.chat"
 :isupport {:chantypes "#&" :prefix "(ov)@+"}}

{:tags    {:time "2024-01-01T00:00:00Z" :msgid "abc123"}
 :source  {:nick "user" :user "ident" :host "host.com"}
 :command "PRIVMSG"
 :params  ["#channel" "Hello world"]}
```

Nothing reads the socket until you ask, so a bot is one loop:

```lisp
(each msg in conn:messages
  (when (= msg:command "PRIVMSG")
    (conn:send "PRIVMSG" (get msg:params 0) "reply")))
```

To do other work while messages arrive, hand them to a queue from a
fiber of their own.

## Invariants

1. `conn:messages` never yields a PING. It answers them itself.
2. `conn:send` prefixes the trailing parameter with `:` when it has to.
3. Registration answers a nick collision by appending `_`, three times
   before it gives up.
