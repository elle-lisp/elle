# Test module for import-file integration tests
(def test-value 42)
(var test-list (list 1 2 3))

# Module exports
(fn [] {:test-var test-var :test-string test-string :test-list test-list})
