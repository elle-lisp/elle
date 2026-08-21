(elle/epoch 12)
## Main-file Unicode generation selection.
## The declaration below selects generation 16. Under the Unicode 16
## tables the codepoint U+10EFA (the second character of the two-character
## literal below) is unassigned, so it does not extend the preceding base.
(unicode! 16)
(assert (= (unicode!) [16 0 0]) "declaration selected Unicode 16.0")
(assert (= (length "a𐻺") 2) "U+10EFA does not extend under Unicode 16")
