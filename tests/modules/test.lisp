(elle/epoch 6)
# Test module for import-file integration tests
(def test-var 42)
(def test-string "hello")
(var test-list (list 1 2 3))

# Module exports
(fn [] {:test-var test-var :test-string test-string :test-list test-list})
