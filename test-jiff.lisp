(def jiff (import "target/release/libelle_jiff.so"))

# destructure what we need
(def {:date date :time time :datetime datetime :timestamp timestamp
      :now now :zoned zoned :span span :signed-duration signed-duration
      :temporal/string tstr :temporal/format tfmt :temporal/compare tcmp
      :temporal/before? before? :temporal/equal? equal?
      :date/year year :date/month month :date/day day
      :date/weekday weekday :date/leap-year? leap?
      :time/hour hour :time/minute minute :time/second second
      :date/add date-add :date/parse date-parse
      :date/start-of-month som :date/end-of-month eom
      :date/next-weekday next-wd
      :temporal/series series
      :span/get span-get :span->struct span->struct
      :tz-system tz-sys :tz-valid? tz-ok?
      :date? date? :temporal? temporal?
      :timestamp/->epoch-millis epoch-ms
      :timestamp/from-epoch-millis from-ms
      } jiff)

(print "--- constructors ---\n")
(def d (date 2024 6 19))
(print "date: " (tstr d) "\n")
(def t (time 15 22 45))
(print "time: " (tstr t) "\n")
(def dt (datetime 2024 6 19 15 22 45))
(print "datetime: " (tstr dt) "\n")
(def ts (timestamp))
(print "timestamp: " (tstr ts) "\n")
(def z (now))
(print "now: " (tstr z) "\n")
(def s (span {:hours 1 :minutes 30}))
(print "span: " (tstr s) "\n")
(def sd (signed-duration 3600))
(print "signed-dur: " (tstr sd) "\n")

(print "\n--- predicates ---\n")
(print "date? " (date? d) "\n")
(print "temporal? " (temporal? d) "\n")
(print "temporal? 42: " (temporal? 42) "\n")

(print "\n--- accessors ---\n")
(print "year: " (year d) "\n")
(print "month: " (month d) "\n")
(print "day: " (day d) "\n")
(print "weekday: " (weekday d) "\n")
(print "hour: " (hour t) "\n")
(print "leap-year?: " (leap? d) "\n")

(print "\n--- arithmetic ---\n")
(def d2 (date-add d (span {:days 30})))
(print "date + 30d: " (tstr d2) "\n")
(print "compare: " (tcmp d d2) "\n")
(print "before?: " (before? d d2) "\n")
(print "equal?: " (equal? d d) "\n")

(print "\n--- parsing ---\n")
(def pd (date-parse "2024-12-25"))
(print "parsed: " (tstr pd) "\n")

(print "\n--- calendar ---\n")
(print "start-of-month: " (tstr (som d)) "\n")
(print "end-of-month: " (tstr (eom d)) "\n")
(print "next monday: " (tstr (next-wd d :monday)) "\n")

(print "\n--- series ---\n")
(def months (series d (span {:months 1}) 4))
(each m in months
  (print "  " (tstr m) "\n"))

(print "\n--- timezone ---\n")
(print "valid?: " (tz-ok? "America/New_York") "\n")

(print "\n--- formatting ---\n")
(print "formatted: " (tfmt "%B %d, %Y" d) "\n")

(print "\n--- epoch roundtrip ---\n")
(def ms (epoch-ms ts))
(print "epoch-ms: " ms "\n")
(def ts2 (from-ms ms))
(print "roundtrip: " (tstr ts2) "\n")

(print "\n--- span inspection ---\n")
(print "hours: " (span-get s :hours) "\n")
(print "minutes: " (span-get s :minutes) "\n")
(print "struct: " (span->struct s) "\n")

(print "\ndone!\n")
