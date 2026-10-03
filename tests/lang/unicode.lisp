(elle/epoch 12)
## Unicode version specification
##
## unicode! is the compile-time surface for the segmentation generation.
## With no arguments it folds to the selected generation's version. With
## arguments it declares the generation this source assumes; the analyzer
## checks the declaration against the locked generation and the form
## evaluates to nil. This file runs in the shared corpus process, which
## uses the default (newest) generation; selection of other generations
## is covered by tests/integration/unicode_generation.rs.

(unicode! 17)

(assert (= (unicode!) [17 0 0])
        "zero-arg unicode! folds to the selected version")

(assert (nil? (unicode! 17)) "matching major declaration evaluates to nil")
(assert (nil? (unicode! 17 0))
        "matching major.minor declaration evaluates to nil")
(assert (nil? (unicode! 17 0 0))
        "matching major.minor.patch declaration evaluates to nil")

# The runtime introspection surface agrees with the compile-time fold.
(assert (= (vm/config :unicode) (unicode!))
        "vm/config :unicode reports the locked generation")
