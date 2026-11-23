# Signatures and calls

<!-- audited: 2026-11-23 -->

What a function's signature says about each parameter and about its result, and how a call reads it.

[The ownership rules](ownership.md) cover every form but a call and a return.
[The memory model](memory.md) defines an owner, a borrow and a move.

## The signature

Each function has an inferred signature, and the programmer writes none. A
call to a known binding reads the callee's signature.

A parameter has one of four modes:

| Mode | The callee | The call |
|------|------------|----------|
| Borrowed | reads the value and keeps nothing | lends the value for the call |
| Sunk | holds the release of the value | passes a value its fiber owns, at that value's last use |
| Returned | hands the value to its result | reads the result as the argument itself, or as an alias of it |
| Stored | stores the value into a named place | moves the value to that place |

The fiber owns a sunk value throughout. The mode says which function's code
holds the release.

A result has one of four kinds, and a fresh result may also borrow:

| Kind | Meaning |
|------|---------|
| Fresh | the call made the value, and the caller's fiber owns it |
| Itself | the value is one parameter's argument, which the callee returns on every path and does not store |
| Alias of a parameter | the value is borrowed from that argument's owner |
| Borrow through a free name | the value is borrowed from an owner the function reaches through a free name |

A free name is a name the function does not bind: a module-level definition
or a capture. The closure that `make` returns, in the examples after the rules,
reads an element through one.

The inner function does not bind `arr`, so `arr` is a free name of it.
`(get arr 0)` borrows the element from `arr`. The inner function returns that
borrow, so its result is a borrow through `arr`, and it is neither fresh nor an
alias of a parameter. `arr` is a capture, so under the Free-name borrow rule
the result borrows from the closure itself.

## The rules

| Rule | Statement |
|------|-----------|
| Call | A call reads the callee's signature for each argument: a borrowed parameter lends the value for the call, a stored parameter moves it to the place the signature names, and a returned parameter hands it to the result. |
| Stored parameter | A parameter is stored where, owned by the fiber on entry, each chain of its owners and hand-overs at each return reaches another parameter, a free name, or a returned value the fiber holds. |
| One place | A function whose parameter is stored into two places is refused. |
| Free-name store | A call that stores an argument into a definition makes that definition a free name of the caller, and a function that stores a parameter into one of its captures stores it into itself. |
| Free-name borrow | A call whose result borrows through a definition makes that definition a free name of the caller, and a function whose result borrows through one of its captures borrows from itself. |
| Returned itself | Where a callee returns one parameter itself on every path and does not store it, a call at that argument's last use hands it to the result. |
| Return | A returned value the call made, or borrowed from a container the call made, makes the result fresh; a value borrowed from a parameter makes it an alias of that parameter; and a value borrowed through a free name makes it a borrow from that owner. |
| Fresh result that borrows | A fresh result may also borrow from a parameter, or from an owner the function reaches through a free name; the signature names each one, and that owner's release waits for the result's last use. |
| Group borrows | The rounds derive borrows with the other result facts; in each round, a call into the group waits as the round before's borrows facts say, under the Fresh result that borrows rule. |
| Pending member | A call into a member that returned nothing in the round before is a call that does not return, as in the first round. |
| Group stores | A group derives stored in steps: each step reruns the rounds with the stored facts of the step before and keeps each fact it derives again, and the first step stores each argument of a call into the group in an unnamed place. A fact still unnamed when the steps stop belongs to a member that never returns, and is dropped. |
| Sink | A parameter the callee neither stores nor returns is lent or sunk, by the call site's choice: a call sinks a value its fiber owns where the call is that value's last use, and the callee's code then releases it at its last use. |
| Aliased arguments | A call sinks an argument at its last use even where another argument of the same call borrows from it; the instance the call names says which parameters borrow from the sunk one, and the callee's code releases the sunk parameter at the last use of it and of each of them. |
| Owner not passed | Where a call passes borrows of a value and not the value, and the call is that value's last use, the call sinks the value as an extra argument of the instance it names, and the callee's code releases it at the last use of the parameters that borrow from it. |

[The region IR](ir.md) says what an instance is. [Calls in Datalog](call.md)
writes the Call and Returned itself rules, and [return](return.md) the Return
rule. [Stored](stored.md) derives the Stored mode under the Stored parameter
rule, and [place](place.md) writes the callee's side of that rule and the One
place target. [Free](free.md) writes the Free-name store rule, and
[through](through.md) the Free-name borrow rule. [Results that
borrow](borrows.md) writes the Fresh result that borrows rule, generalized
there to every result but a parameter itself.
[Group](group.md) writes the Group borrows rule, and [recursion](recursion.md)
the Pending member rule. [Steps](steps.md) writes the Group stores rule.
[Itself](itself.md) derives the Itself kind. [Sink](sink.md) writes the Sink
rule, where a parameter that a result borrows from counts as returned.
[Aliased arguments](aliased.md) writes the Aliased arguments rule, and
[Owner not passed](owner.md) the Owner not passed rule.

A fresh result that borrows is what `keep-first` returns. The pair is new, and
its head is borrowed from the owner of `xs`, so that owner waits for `r`.

A sunk argument is what `count-up` receives. The array's last use is inside
the callee, so the callee's code releases it there and the caller holds no
release.

In `(g x (first x))` the second argument borrows from the first, and in
`(reduce f (first xs) (rest xs))` the call passes two borrows of `xs` and not
`xs`. Where either call is the owner's last use, the Aliased arguments rule
and the Owner not passed rule place its release in the callee. The examples run
as the fences here:

```lisp
(fn []
  (def {:example example} ((import "../spec.md")))

  (defn make []
    (def arr @[@[1]])
    (fn [] (get arr 0)))
  (defn keep-first [c] (pair (first c) ()))
  (defn count-up [a] (+ (length a) 1))
  (defn three [] (count-up (array 1 2)))

  {:fences [(example 'read-first :doc "The closure reads the element of arr."
              :check (fn [] (def read-first (make)) (= (get (read-first) 0) 1)))
            (example 'keep-first :doc "The new pair holds the first element of xs."
              :check (fn [] (def xs (list [1 2] [3 4])) (= (first (keep-first xs)) [1 2])))
            (example 'count-up :doc "The callee reads the array it was handed."
              :check (fn [] (= (three) 3)))]})
```

## A callee the compiler cannot read

A call whose callee's signature the compiler cannot read is refused, whatever
it passes. That covers a closure read from a mutable binding, from a
container, or through `module:field`. Every `module:field` call is such a call
today ([#1232](https://github.com/elle-lisp/elle/issues/1232)).

Each native declares its signature, as the next section says, and a call to a
native with no declaration is refused. Every plugin primitive is refused until
the plugin ABI carries a signature.

One rule for a call to a parameter is a target
([the judgment](judgment.md)): the call records a requirement on that
argument's signature in the function's own signature. Each call site checks
the requirement at compile time, and an argument that does not fit is refused.

## A native's signature

A native's body is not Elle, so no rule reads it, and the native declares its
signature instead. A native holds no release, so no parameter of one is sunk.

| Rule | Statement |
|------|-----------|
| Native modes | A native's spec signature gives each parameter one mode: borrowed, stored into a place, returned, crosses (passed to another fiber) or resumed (a fiber the call resumes). |
| Native result | A native's result has one kind: immediate, fresh, alias, itself, or the payload of one parameter; a returned parameter is what an alias or a fresh result borrows from, and itself names exactly one returned parameter. |
| Native mutable result | A native's fresh result is immutable unless the native declares it mutable, and only a fresh result is declared mutable. |
| Mutable result borrows nothing | A fresh mutable result borrows from no parameter, so its native returns none. |
| Native places | A native stores a parameter into another parameter, `[:param j]`, or into its fresh result, `:result`; a native that stores into anything else has no declaration. |
| Native rest | A declaration has a rest mode exactly when the native's arity has no fixed count, and lists one mode per fixed parameter; a unit test holds every declaration to its native's arity. |

A crossing parameter moves its value to another fiber, as an emit does
([crossing](crossing.md)), and a payload result is what the fiber value holds
([payload](payload.md)). An immediate result holds no heap value.
`fiber/resume` declares its fiber resumed, its value crossing, and its result
the payload of its fiber, and `fiber/value` declares the same result.

A declaration holds on every path the native takes. A native that dispatches
on a type or a trait answers a different kind on each path, so `first`,
`rest`, `length` and `get` declare nothing, and a call to one is refused.

## Not decided

- How a call to a parameter names an instance of its callee.
- What a signature says of a native that removes an element from an argument.
