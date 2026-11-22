(elle/epoch 18)
# audited: 2026-11-21
# A window with a count and two buttons that change it, redrawn each frame.
# demos/egui/README.md

(def egui-plugin (import "plugin/egui"))
(def ui ((import "std/egui") egui-plugin))

(def @count 0)

(def win (ui:open :title "Counter" :width 300 :height 150))
(ui:run win
        (fn [ix]
          (when (ui:clicked? ix :inc) (assign count (inc count)))
          (when (ui:clicked? ix :dec) (assign count (dec count)))
          (ui:v-layout (ui:heading (string "Count: " count))
                       (ui:h-layout (ui:button :dec "-") (ui:button :inc "+")))))
