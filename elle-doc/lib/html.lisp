(var html-escape
  (fn (str)
    (var s1 (string-replace str "&" "&amp;"))
    (var s2 (string-replace s1 "<" "&lt;"))
    (var s3 (string-replace s2 ">" "&gt;"))
    (var s4 (string-replace s3 "\"" "&quot;"))
    (string-replace s4 "'" "&#39;")))

(display (html-escape "test"))
(newline)
