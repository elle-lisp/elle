# Elle

It's 2026. I talk to Claude. Soon, I'll talk to it the way I talk to a person -
out loud, in real time.

When that happens, I want my AI collaborator to be able to write and run
programs as fast as it thinks. Not scaffolded around a slow language with
ceremony and boilerplate. Not constrained by a sandboxed REPL. I want it to
reach directly into the machine - any shared library on the system, any C ABI,
any hardware - and act.

The problem with existing languages is that every one of them is a fossil
record of compromises: for human readability, for human development workflows,
for interop with other human-written software, and for backward compatibility
with all of the above going back decades. That's not a criticism. That's what
languages are for - or were.

Elle is built for a different set of constraints. It's a Lisp, because
obviously. Simple syntax. No backward compatibility baggage - ever. A real FFI
that doesn't make you work for it. And an effect system: right now that means
the runtime knows, at a fine grain, what any piece of code can and cannot do.
That's the first control surface. I expect it won't be the last.

The old objection to writing your own language was that it would have one user
and no libraries. Now you can speak to a C library. The language just has to
be fast to get in and out of. If it's slow today, it'll be faster tomorrow. If
this is the fastest there is, what more do you want?

Fast. Simple. Powerful. It's what you want in a language.

## License
MIT
