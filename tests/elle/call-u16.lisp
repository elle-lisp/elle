(elle/epoch 7)
# Regression test: Call arg count must be u16, not u8.
#
# Struct literals desugar to a call to the `struct` primitive.
# A struct with >128 key-value pairs = >256 arguments. When the
# Call instruction used u8 for arg count, this silently truncated,
# producing a struct with the wrong number of fields.

# Build a struct with 130 key-value pairs = 260 call arguments
(def big {:k0 0 :k1 1 :k2 2 :k3 3 :k4 4 :k5 5 :k6 6 :k7 7 :k8 8 :k9 9
          :k10 10 :k11 11 :k12 12 :k13 13 :k14 14 :k15 15 :k16 16 :k17 17 :k18 18 :k19 19
          :k20 20 :k21 21 :k22 22 :k23 23 :k24 24 :k25 25 :k26 26 :k27 27 :k28 28 :k29 29
          :k30 30 :k31 31 :k32 32 :k33 33 :k34 34 :k35 35 :k36 36 :k37 37 :k38 38 :k39 39
          :k40 40 :k41 41 :k42 42 :k43 43 :k44 44 :k45 45 :k46 46 :k47 47 :k48 48 :k49 49
          :k50 50 :k51 51 :k52 52 :k53 53 :k54 54 :k55 55 :k56 56 :k57 57 :k58 58 :k59 59
          :k60 60 :k61 61 :k62 62 :k63 63 :k64 64 :k65 65 :k66 66 :k67 67 :k68 68 :k69 69
          :k70 70 :k71 71 :k72 72 :k73 73 :k74 74 :k75 75 :k76 76 :k77 77 :k78 78 :k79 79
          :k80 80 :k81 81 :k82 82 :k83 83 :k84 84 :k85 85 :k86 86 :k87 87 :k88 88 :k89 89
          :k90 90 :k91 91 :k92 92 :k93 93 :k94 94 :k95 95 :k96 96 :k97 97 :k98 98 :k99 99
          :k100 100 :k101 101 :k102 102 :k103 103 :k104 104
          :k105 105 :k106 106 :k107 107 :k108 108 :k109 109
          :k110 110 :k111 111 :k112 112 :k113 113 :k114 114
          :k115 115 :k116 116 :k117 117 :k118 118 :k119 119
          :k120 120 :k121 121 :k122 122 :k123 123 :k124 124
          :k125 125 :k126 126 :k127 127 :k128 128 :k129 129})

(assert (= (length (keys big)) 130) "struct has 130 keys")
(assert (= big:k0 0)   "first key")
(assert (= big:k129 129) "last key")
(assert (= big:k64 64)  "middle key")

(println "call-u16: all tests passed")
