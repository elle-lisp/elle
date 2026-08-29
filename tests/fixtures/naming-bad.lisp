(elle/epoch 12)
# Non-kebab naming conventions — a zero-diagnostic lint corpus since the
# kebab-case naming lint was removed. Bindings are immutable so no mutability
# lint fires, and the trailing struct reads every one of them so no
# unused-binding lint fires. Only the names are the subject.

(def myVariable 10)
(def camelCase 42)
(def PascalCase 100)
(def snake_case 5)

{:my-variable myVariable
 :camel-case camelCase
 :pascal-case PascalCase
 :snake-case snake_case}
