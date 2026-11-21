# Signatures and calls

<!-- audited: 2026-11-20 -->

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
or a capture. The closure below reads an element through one:

```lisp
(defn make []
  (def arr @[@[1]])
  (fn [] (get arr 0)))

(def read-first (make))
(assert (= (get (read-first) 0) 1) "the closure reads the element of arr")
```

The inner function does not bind `arr`, so `arr` is a free name of it.
`(get arr 0)` borrows the element from `arr`. The inner function returns that
borrow, so its result is a borrow from `arr`, and it is neither fresh nor an
alias of a parameter.

## The rules

| Rule | Statement |
|------|-----------|
| Call | A call reads the callee's signature for each argument: a borrowed parameter lends the value for the call, a stored parameter moves it to the place the signature names, and a returned parameter hands it to the result. |
| Stored parameter | A parameter is stored where, owned by the fiber on entry, each chain of its owners and hand-overs at each return reaches another parameter, a free name, or a returned value the fiber holds. |
| One place | A function whose parameter is stored into two places is refused. |
| Returned itself | Where a callee returns one parameter itself on every path and does not store it, a call at that argument's last use hands it to the result, as a recur hands a value on. |
| Return | A returned value the call made, or borrowed from a container the call made, makes the result fresh; a value borrowed from a parameter makes it an alias of that parameter; and a value borrowed through a free name makes it a borrow from that owner. |
| Fresh result that borrows | A fresh result may also borrow from a parameter, or from an owner the function reaches through a free name; the signature names each one, and that owner's release waits for the result's last use. |
| Group borrows | The rounds derive borrows with the other result facts; in each round, a call into the group waits as the round before's borrows facts say, under the Fresh result that borrows rule. |
| Pending member | A call into a member that returned nothing in the round before is a call that does not return, as in the first round. |
| Sink | A parameter the callee neither stores nor returns is lent or sunk, by the call site's choice: a call sinks a value its fiber owns where the call is that value's last use, and the callee's code then releases it at its last use. |
| Aliased arguments | A call sinks an argument at its last use even where another argument of the same call borrows from it; the instance the call names says which parameters borrow from the sunk one, and the callee's code releases the sunk parameter at the last use of it and of each of them. |
| Owner not passed | Where a call passes borrows of a value and not the value, and the call is that value's last use, the call sinks the value as an extra argument of the instance it names, and the callee's code releases it at the last use of the parameters that borrow from it. |

[The region IR](ir.md) says what an instance is. [Calls in Datalog](call.md)
writes the Call and Returned itself rules, and [return](return.md) the Return
rule. [Stored](stored.md) derives the Stored mode under the Stored parameter
rule, and [place](place.md) writes the callee's side of that rule and the One
place target. [Results that borrow](borrows.md) writes the Fresh result that
borrows rule, generalized there to every result but a parameter itself.
[Group](group.md) writes the Group borrows rule, and [recursion](recursion.md)
the Pending member rule. [Itself](itself.md) derives the Itself kind. [Sink](sink.md) writes the Sink rule, where a parameter that a
result borrows from counts as returned.
[Aliased arguments](aliased.md) writes the Aliased arguments rule, and
[Owner not passed](owner.md) the Owner not passed rule.

A fresh result that borrows is what `keep-first` returns. The pair is new, and
its head is borrowed from the owner of `xs`, so that owner waits for `r`:

```lisp
(defn keep-first [c] (pair (first c) ()))

(def xs (list [1 2] [3 4]))
(def r (keep-first xs))
(assert (= (first r) [1 2]) "the new pair holds the first element of xs")
```

A sunk argument is what `count-up` receives. The array's last use is inside
the callee, so the callee's code releases it there and the caller holds no
release:

```lisp
(defn count-up [a] (+ (length a) 1))
(defn three [] (count-up (array 1 2)))

(assert (= (three) 3) "the callee reads the array it was handed")
```

In `(g x (first x))` the second argument borrows from the first, and in
`(reduce f (first xs) (rest xs))` the call passes two borrows of `xs` and not
`xs`. Where either call is the owner's last use, the Aliased arguments rule
and the Owner not passed rule place its release in the callee.

## A callee the compiler cannot read

A call whose callee's signature the compiler cannot read is refused, whatever
it passes. That covers a closure read from a mutable binding, from a
container, or through `module:field`. Every `module:field` call is such a call
today ([#1232](https://github.com/elle-lisp/elle/issues/1232)).

Each native declares its signature in the same vocabulary, and a call to a
native with no declaration is refused. Every plugin primitive is refused until
the plugin ABI carries a signature.

One rule for a call to a parameter is a target
([the judgment](judgment.md)): the call records a requirement on that
argument's signature in the function's own signature. Each call site checks
the requirement at compile time, and an argument that does not fit is refused.

## Not decided

- How a call to a parameter names an instance of its callee.
- What a signature says of a native that removes an element from an argument.
- What a native declares whose result is fresh on an immutable argument and
  the argument itself on a mutable one, as `push` does.
