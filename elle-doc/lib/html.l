(define html-escape
  (lambda (str)
    (define s1 (string-replace str "&" "&amp;"))
    (define s2 (string-replace s1 "<" "&lt;"))
    (define s3 (string-replace s2 ">" "&gt;"))
    (define s4 (string-replace s3 "\"" "&quot;"))
    (string-replace s4 "'" "&#39;")))

(display (html-escape "test"))
(newline)
