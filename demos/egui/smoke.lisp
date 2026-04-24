(elle/epoch 9)
## demos/egui/smoke.lisp — headless smoke test for egui plugin

(def egui-plugin (import "plugin/egui"))
(def ui ((import "std/egui") egui-plugin))

(def win (ui:open :title "Smoke Test"))
(assert (ui:open? win) "window should be open")

(def ix (ui:frame win (ui:v-layout
  (ui:heading "Hello from Elle!")
  (ui:button :ok "OK"))))

(assert (= ix:closed false) "should not be closed")
(assert (= (length ix:size) 2) "size should be [w h]")

(ui:close win)
(assert (not (ui:open? win)) "window should be closed")

(println "egui smoke test passed")
