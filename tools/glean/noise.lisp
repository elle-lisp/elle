(elle/epoch 18)
# audited: 2026-11-23
# Glean's noise: the :glean/noise signal it raises at a fact the hand facts model otherwise, and the mediator that hears it.
# tools/glean/census.md

(fn []
  (signal :glean/noise)

  (defn noise [cause id point values]
    "Raise :glean/noise for CAUSE in the function of closure id ID, at POINT,
  naming VALUES. The resume value is ignored."
    (emit :glean/noise {:cause cause :id id :point point :values (freeze values)})
    nil)

  (defn heard [thunk]
    "[value payloads]: THUNK's value, and the payload of each :glean/noise
  signal it raises, in order. Every other signal passes."
    (def payloads @[])
    (def f (fiber/new thunk |:glean/noise|))
    (var value (fiber/resume f))
    (while (= (fiber/status f) :paused)
      (push payloads value)
      (assign value (fiber/resume f nil)))
    [value (freeze payloads)])

  {:noise noise :heard heard})
