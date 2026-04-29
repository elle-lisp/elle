(elle/epoch 9)
## lib/gtk4/bind.lisp — FFI bindings and low-level helpers for GTK4, GLib, GObject, WebKit
##
## Raw ffi/defbind declarations plus glib-wait (scheduler integration)
## and run-app (GApplication event loop).

(fn []

  # ── Shared libraries ──────────────────────────────────────────────

  (def libgtk (ffi/native "libgtk-4.so.1"))
  (def libglib (ffi/native "libglib-2.0.so"))
  (def libgobj (ffi/native "libgobject-2.0.so"))
  (def libwebkit (ffi/native "libwebkitgtk-6.0.so.4"))
  (def libjsc (ffi/native "libjavascriptcoregtk-6.0.so.1"))

  # ── GTK init ──────────────────────────────────────────────────────

  (ffi/defbind gtk-init libgtk "gtk_init" :void [])

  # ── GtkWindow ─────────────────────────────────────────────────────

  (ffi/defbind gtk-window-new libgtk "gtk_window_new" :ptr [])
  (ffi/defbind gtk-window-present libgtk "gtk_window_present" :void [:ptr])
  (ffi/defbind gtk-window-close libgtk "gtk_window_close" :void [:ptr])
  (ffi/defbind gtk-window-destroy libgtk "gtk_window_destroy" :void [:ptr])
  (ffi/defbind gtk-window-set-title libgtk "gtk_window_set_title"
               :void [:ptr :string])
  (ffi/defbind gtk-window-set-default-size libgtk "gtk_window_set_default_size"
               :void [:ptr :int :int])
  (ffi/defbind gtk-window-fullscreen libgtk "gtk_window_fullscreen" :void [:ptr])
  (ffi/defbind gtk-window-set-child libgtk "gtk_window_set_child"
               :void [:ptr :ptr])

  # ── GtkWidget ─────────────────────────────────────────────────────

  (ffi/defbind gtk-widget-show libgtk "gtk_widget_show" :void [:ptr])
  (ffi/defbind gtk-widget-hide libgtk "gtk_widget_hide" :void [:ptr])
  (ffi/defbind gtk-widget-set-visible libgtk "gtk_widget_set_visible"
               :void [:ptr :int])
  (ffi/defbind gtk-widget-set-sensitive libgtk "gtk_widget_set_sensitive"
               :void [:ptr :int])
  (ffi/defbind gtk-widget-set-size-request libgtk "gtk_widget_set_size_request"
               :void [:ptr :int :int])
  (ffi/defbind gtk-widget-set-hexpand libgtk "gtk_widget_set_hexpand"
               :void [:ptr :int])
  (ffi/defbind gtk-widget-set-vexpand libgtk "gtk_widget_set_vexpand"
               :void [:ptr :int])
  (ffi/defbind gtk-widget-add-css-class libgtk "gtk_widget_add_css_class"
               :void [:ptr :string])
  (ffi/defbind gtk-widget-set-margin-start libgtk "gtk_widget_set_margin_start"
               :void [:ptr :int])
  (ffi/defbind gtk-widget-set-margin-end libgtk "gtk_widget_set_margin_end"
               :void [:ptr :int])
  (ffi/defbind gtk-widget-set-margin-top libgtk "gtk_widget_set_margin_top"
               :void [:ptr :int])
  (ffi/defbind gtk-widget-set-margin-bottom libgtk
               "gtk_widget_set_margin_bottom" :void [:ptr :int])
  (ffi/defbind gtk-widget-set-halign libgtk "gtk_widget_set_halign"
               :void [:ptr :int])
  (ffi/defbind gtk-widget-set-valign libgtk "gtk_widget_set_valign"
               :void [:ptr :int])
  (ffi/defbind gtk-widget-unparent libgtk "gtk_widget_unparent" :void [:ptr])
  (ffi/defbind gtk-widget-queue-draw libgtk "gtk_widget_queue_draw" :void [:ptr])
  (ffi/defbind gtk-widget-add-controller libgtk "gtk_widget_add_controller"
               :void [:ptr :ptr])

  # ── GtkDrawingArea ──────────────────────────────────────────────

  (ffi/defbind gtk-drawing-area-new libgtk "gtk_drawing_area_new" :ptr [])
  (ffi/defbind gtk-drawing-area-set-content-width libgtk
               "gtk_drawing_area_set_content_width" :void [:ptr :int])
  (ffi/defbind gtk-drawing-area-set-content-height libgtk
               "gtk_drawing_area_set_content_height" :void [:ptr :int])
  (ffi/defbind gtk-drawing-area-set-draw-func libgtk
               "gtk_drawing_area_set_draw_func" :void [:ptr :ptr :ptr :ptr])

  # ── GtkApplication ──────────────────────────────────────────────

  (ffi/defbind gtk-application-new libgtk "gtk_application_new"
               :ptr [:string :u32])
  (ffi/defbind gtk-application-window-new libgtk "gtk_application_window_new"
               :ptr [:ptr])

  # ── Event controllers ───────────────────────────────────────────

  (ffi/defbind gtk-gesture-click-new libgtk "gtk_gesture_click_new" :ptr [])
  (ffi/defbind gtk-gesture-single-set-button libgtk
               "gtk_gesture_single_set_button" :void [:ptr :u32])
  (ffi/defbind gtk-gesture-single-get-current-button libgtk
               "gtk_gesture_single_get_current_button" :u32 [:ptr])
  (ffi/defbind gtk-event-controller-scroll-new libgtk
               "gtk_event_controller_scroll_new" :ptr [:u32])
  (ffi/defbind gtk-event-controller-key-new libgtk
               "gtk_event_controller_key_new" :ptr [])

  # ── GtkBox ────────────────────────────────────────────────────────

  (ffi/defbind gtk-box-new libgtk "gtk_box_new" :ptr [:int :int])
  (ffi/defbind gtk-box-append libgtk "gtk_box_append" :void [:ptr :ptr])
  (ffi/defbind gtk-box-remove libgtk "gtk_box_remove" :void [:ptr :ptr])

  # ── GtkLabel ──────────────────────────────────────────────────────

  (ffi/defbind gtk-label-new libgtk "gtk_label_new" :ptr [:string])
  (ffi/defbind gtk-label-set-text libgtk "gtk_label_set_text"
               :void [:ptr :string])
  (ffi/defbind gtk-label-get-text libgtk "gtk_label_get_text" :ptr [:ptr])
  (ffi/defbind gtk-label-set-xalign libgtk "gtk_label_set_xalign"
               :void [:ptr :float])
  (ffi/defbind gtk-label-set-wrap libgtk "gtk_label_set_wrap" :void [:ptr :int])

  # ── GtkButton ─────────────────────────────────────────────────────

  (ffi/defbind gtk-button-new-with-label libgtk "gtk_button_new_with_label"
               :ptr [:string])
  (ffi/defbind gtk-button-set-label libgtk "gtk_button_set_label"
               :void [:ptr :string])
  (ffi/defbind gtk-button-get-label libgtk "gtk_button_get_label" :ptr [:ptr])

  # ── GtkToggleButton ──────────────────────────────────────────────

  (ffi/defbind gtk-toggle-button-new libgtk "gtk_toggle_button_new" :ptr [])
  (ffi/defbind gtk-toggle-button-new-with-label libgtk
               "gtk_toggle_button_new_with_label" :ptr [:string])
  (ffi/defbind gtk-toggle-button-set-active libgtk
               "gtk_toggle_button_set_active" :void [:ptr :int])
  (ffi/defbind gtk-toggle-button-get-active libgtk
               "gtk_toggle_button_get_active" :int [:ptr])

  # ── GtkEntry (text-input) ────────────────────────────────────────

  (ffi/defbind gtk-entry-new libgtk "gtk_entry_new" :ptr [])
  (ffi/defbind gtk-entry-set-placeholder-text libgtk
               "gtk_entry_set_placeholder_text" :void [:ptr :string])
  (ffi/defbind gtk-editable-get-text libgtk "gtk_editable_get_text" :ptr [:ptr])
  (ffi/defbind gtk-editable-set-text libgtk "gtk_editable_set_text"
               :void [:ptr :string])

  # ── GtkTextView (text-edit) ──────────────────────────────────────

  (ffi/defbind gtk-text-view-new libgtk "gtk_text_view_new" :ptr [])
  (ffi/defbind gtk-text-view-get-buffer libgtk "gtk_text_view_get_buffer"
               :ptr [:ptr])
  (ffi/defbind gtk-text-view-set-wrap-mode libgtk "gtk_text_view_set_wrap_mode"
               :void [:ptr :int])
  (ffi/defbind gtk-text-view-set-editable libgtk "gtk_text_view_set_editable"
               :void [:ptr :int])
  (ffi/defbind gtk-text-buffer-set-text libgtk "gtk_text_buffer_set_text"
               :void [:ptr :string :int])
  (ffi/defbind gtk-text-buffer-get-start-iter libgtk
               "gtk_text_buffer_get_start_iter" :void [:ptr :ptr])
  (ffi/defbind gtk-text-buffer-get-end-iter libgtk
               "gtk_text_buffer_get_end_iter" :void [:ptr :ptr])
  (ffi/defbind gtk-text-buffer-get-text libgtk "gtk_text_buffer_get_text"
               :ptr [:ptr :ptr :ptr :int])

  # ── GtkCheckButton ───────────────────────────────────────────────

  (ffi/defbind gtk-check-button-new-with-label libgtk
               "gtk_check_button_new_with_label" :ptr [:string])
  (ffi/defbind gtk-check-button-set-active libgtk "gtk_check_button_set_active"
               :void [:ptr :int])
  (ffi/defbind gtk-check-button-get-active libgtk "gtk_check_button_get_active"
               :int [:ptr])

  # ── GtkSwitch ────────────────────────────────────────────────────

  (ffi/defbind gtk-switch-new libgtk "gtk_switch_new" :ptr [])
  (ffi/defbind gtk-switch-set-active libgtk "gtk_switch_set_active"
               :void [:ptr :int])
  (ffi/defbind gtk-switch-get-active libgtk "gtk_switch_get_active" :int [:ptr])

  # ── GtkScale (slider) ────────────────────────────────────────────

  (ffi/defbind gtk-scale-new-with-range libgtk "gtk_scale_new_with_range"
               :ptr [:int :double :double :double])
  (ffi/defbind gtk-range-get-value libgtk "gtk_range_get_value" :double [:ptr])
  (ffi/defbind gtk-range-set-value libgtk "gtk_range_set_value"
               :void [:ptr :double])

  # ── GtkSpinButton ────────────────────────────────────────────────

  (ffi/defbind gtk-spin-button-new-with-range libgtk
               "gtk_spin_button_new_with_range" :ptr [:double :double :double])
  (ffi/defbind gtk-spin-button-get-value libgtk "gtk_spin_button_get_value"
               :double [:ptr])
  (ffi/defbind gtk-spin-button-set-value libgtk "gtk_spin_button_set_value"
               :void [:ptr :double])

  # ── GtkDropDown (combo-box) ──────────────────────────────────────

  (ffi/defbind gtk-string-list-new libgtk "gtk_string_list_new" :ptr [:ptr])
  (ffi/defbind gtk-drop-down-new libgtk "gtk_drop_down_new" :ptr [:ptr :ptr])
  (ffi/defbind gtk-drop-down-get-selected libgtk "gtk_drop_down_get_selected"
               :uint [:ptr])
  (ffi/defbind gtk-drop-down-set-selected libgtk "gtk_drop_down_set_selected"
               :void [:ptr :uint])

  # ── GtkSearchEntry ───────────────────────────────────────────────

  (ffi/defbind gtk-search-entry-new libgtk "gtk_search_entry_new" :ptr [])

  # ── GtkProgressBar ───────────────────────────────────────────────

  (ffi/defbind gtk-progress-bar-new libgtk "gtk_progress_bar_new" :ptr [])
  (ffi/defbind gtk-progress-bar-set-fraction libgtk
               "gtk_progress_bar_set_fraction" :void [:ptr :double])
  (ffi/defbind gtk-progress-bar-get-fraction libgtk
               "gtk_progress_bar_get_fraction" :double [:ptr])

  # ── GtkSpinner ────────────────────────────────────────────────────

  (ffi/defbind gtk-spinner-new libgtk "gtk_spinner_new" :ptr [])
  (ffi/defbind gtk-spinner-start libgtk "gtk_spinner_start" :void [:ptr])
  (ffi/defbind gtk-spinner-stop libgtk "gtk_spinner_stop" :void [:ptr])

  # ── GtkSeparator ──────────────────────────────────────────────────

  (ffi/defbind gtk-separator-new libgtk "gtk_separator_new" :ptr [:int])

  # ── GtkScrolledWindow ────────────────────────────────────────────

  (ffi/defbind gtk-scrolled-window-new libgtk "gtk_scrolled_window_new" :ptr [])
  (ffi/defbind gtk-scrolled-window-set-child libgtk
               "gtk_scrolled_window_set_child" :void [:ptr :ptr])
  (ffi/defbind gtk-scrolled-window-set-min-content-height libgtk
               "gtk_scrolled_window_set_min_content_height" :void [:ptr :int])
  (ffi/defbind gtk-scrolled-window-set-min-content-width libgtk
               "gtk_scrolled_window_set_min_content_width" :void [:ptr :int])

  # ── GtkExpander ───────────────────────────────────────────────────

  (ffi/defbind gtk-expander-new libgtk "gtk_expander_new" :ptr [:string])
  (ffi/defbind gtk-expander-set-child libgtk "gtk_expander_set_child"
               :void [:ptr :ptr])
  (ffi/defbind gtk-expander-set-expanded libgtk "gtk_expander_set_expanded"
               :void [:ptr :int])

  # ── GtkFrame ──────────────────────────────────────────────────────

  (ffi/defbind gtk-frame-new libgtk "gtk_frame_new" :ptr [:string])
  (ffi/defbind gtk-frame-set-child libgtk "gtk_frame_set_child"
               :void [:ptr :ptr])

  # ── GtkGrid ──────────────────────────────────────────────────────

  (ffi/defbind gtk-grid-new libgtk "gtk_grid_new" :ptr [])
  (ffi/defbind gtk-grid-attach libgtk "gtk_grid_attach"
               :void [:ptr :ptr :int :int :int :int])
  (ffi/defbind gtk-grid-set-row-spacing libgtk "gtk_grid_set_row_spacing"
               :void [:ptr :int])
  (ffi/defbind gtk-grid-set-column-spacing libgtk "gtk_grid_set_column_spacing"
               :void [:ptr :int])

  # ── GtkStack / GtkStackPage ──────────────────────────────────────

  (ffi/defbind gtk-stack-new libgtk "gtk_stack_new" :ptr [])
  (ffi/defbind gtk-stack-add-titled libgtk "gtk_stack_add_titled"
               :ptr [:ptr :ptr :string :string])
  (ffi/defbind gtk-stack-set-visible-child-name libgtk
               "gtk_stack_set_visible_child_name" :void [:ptr :string])
  (ffi/defbind gtk-stack-get-visible-child-name libgtk
               "gtk_stack_get_visible_child_name" :ptr [:ptr])

  # ── GtkNotebook ──────────────────────────────────────────────────

  (ffi/defbind gtk-notebook-new libgtk "gtk_notebook_new" :ptr [])
  (ffi/defbind gtk-notebook-append-page libgtk "gtk_notebook_append_page"
               :int [:ptr :ptr :ptr])
  (ffi/defbind gtk-notebook-get-current-page libgtk
               "gtk_notebook_get_current_page" :int [:ptr])
  (ffi/defbind gtk-notebook-set-current-page libgtk
               "gtk_notebook_set_current_page" :void [:ptr :int])

  # ── GtkPaned ─────────────────────────────────────────────────────

  (ffi/defbind gtk-paned-new libgtk "gtk_paned_new" :ptr [:int])
  (ffi/defbind gtk-paned-set-start-child libgtk "gtk_paned_set_start_child"
               :void [:ptr :ptr])
  (ffi/defbind gtk-paned-set-end-child libgtk "gtk_paned_set_end_child"
               :void [:ptr :ptr])

  # ── GtkCenterBox ──────────────────────────────────────────────────

  (ffi/defbind gtk-center-box-new libgtk "gtk_center_box_new" :ptr [])
  (ffi/defbind gtk-center-box-set-start-widget libgtk
               "gtk_center_box_set_start_widget" :void [:ptr :ptr])
  (ffi/defbind gtk-center-box-set-center-widget libgtk
               "gtk_center_box_set_center_widget" :void [:ptr :ptr])
  (ffi/defbind gtk-center-box-set-end-widget libgtk
               "gtk_center_box_set_end_widget" :void [:ptr :ptr])

  # ── GtkOverlay ────────────────────────────────────────────────────

  (ffi/defbind gtk-overlay-new libgtk "gtk_overlay_new" :ptr [])
  (ffi/defbind gtk-overlay-set-child libgtk "gtk_overlay_set_child"
               :void [:ptr :ptr])
  (ffi/defbind gtk-overlay-add-overlay libgtk "gtk_overlay_add_overlay"
               :void [:ptr :ptr])

  # ── GtkRevealer ──────────────────────────────────────────────────

  (ffi/defbind gtk-revealer-new libgtk "gtk_revealer_new" :ptr [])
  (ffi/defbind gtk-revealer-set-child libgtk "gtk_revealer_set_child"
               :void [:ptr :ptr])
  (ffi/defbind gtk-revealer-set-reveal-child libgtk
               "gtk_revealer_set_reveal_child" :void [:ptr :int])
  (ffi/defbind gtk-revealer-set-transition-type libgtk
               "gtk_revealer_set_transition_type" :void [:ptr :int])

  # ── GtkImage ──────────────────────────────────────────────────────

  (ffi/defbind gtk-image-new-from-file libgtk "gtk_image_new_from_file"
               :ptr [:string])
  (ffi/defbind gtk-image-new-from-icon-name libgtk
               "gtk_image_new_from_icon_name" :ptr [:string])
  (ffi/defbind gtk-image-set-pixel-size libgtk "gtk_image_set_pixel_size"
               :void [:ptr :int])

  # ── GtkCalendar ──────────────────────────────────────────────────

  (ffi/defbind gtk-calendar-new libgtk "gtk_calendar_new" :ptr [])

  # ── CSS ───────────────────────────────────────────────────────────

  (ffi/defbind gtk-css-provider-new libgtk "gtk_css_provider_new" :ptr [])
  (ffi/defbind gtk-css-provider-load-from-string libgtk
               "gtk_css_provider_load_from_string" :void [:ptr :string])
  (ffi/defbind gtk-style-context-add-provider-for-display libgtk
               "gtk_style_context_add_provider_for_display"
               :void [:ptr :ptr :uint])
  (ffi/defbind gdk-display-get-default libgtk "gdk_display_get_default" :ptr [])

  # ── GLib main loop ────────────────────────────────────────────────

  (ffi/defbind g-main-context-default libglib "g_main_context_default" :ptr [])
  (ffi/defbind g-main-context-iteration libglib "g_main_context_iteration"
               :int [:ptr :int])
  (ffi/defbind g-main-context-pending libglib "g_main_context_pending"
               :int [:ptr])
  (ffi/defbind g-main-context-prepare libglib "g_main_context_prepare"
               :int [:ptr :ptr])
  (ffi/defbind g-main-context-query libglib "g_main_context_query"
               :int [:ptr :int :ptr :ptr :int])
  (ffi/defbind g-main-context-check libglib "g_main_context_check"
               :int [:ptr :int :ptr :int])
  (ffi/defbind g-main-context-dispatch libglib "g_main_context_dispatch"
               :void [:ptr])

  (def MAX_POLL_FDS 64)
  (def GPOLLFD_SIZE 8)

  (defn glib-wait (ctx)
    "Yield to Elle's scheduler until GLib has events ready, then dispatch.
   Uses prepare/query/check/dispatch with ev/poll-fd on the primary fd."
    (ffi/with-stack [[priority 4] [timeout 4]
                     [fds (* MAX_POLL_FDS GPOLLFD_SIZE)]]
                    (g-main-context-prepare ctx priority)
                    (let* [pri (ffi/read priority :int)
                           nfds (g-main-context-query ctx pri timeout fds
                           MAX_POLL_FDS)
                           tms (ffi/read timeout :int)]
                      (each i in (range nfds)
                        (ffi/write (ptr/add fds (+ (* i GPOLLFD_SIZE) 6)) :u16 0))  # Block on the primary fd or respect the timeout
                      (when (not (zero? tms))
                        (if (> nfds 0)
                          (let* [fd0 (ffi/read fds :int)
                                 tsec (if (< tms 0) 60.0 (/ tms 1000.0))
                                 revents (ev/poll-fd fd0 :read-write tsec)]
                            (when (> revents 0)
                              (ffi/write (ptr/add fds 6) :u16 revents)))
                          (ev/sleep (if (> tms 0) (/ tms 1000.0) 0))))
                      (when (nonzero? (g-main-context-check ctx pri fds nfds))
                        (g-main-context-dispatch ctx)))))

  # ── GApplication ─────────────────────────────────────────────────

  (def libgio (ffi/native "libgio-2.0.so.0"))
  (ffi/defbind g-application-register libgio "g_application_register"
               :int [:ptr :ptr :ptr])
  (ffi/defbind g-application-activate libgio "g_application_activate"
               :void [:ptr])
  (ffi/defbind g-application-run libgio "g_application_run"
               :int [:ptr :int :ptr])

  (ffi/defbind g-application-quit libgio "g_application_quit" :void [:ptr])

  (defn run-app [app &named @quit]
    "Cooperative GTK event loop. Registers and activates the app, then
   blocks on GLib's event sources via ev/poll-fd, yielding to Elle's
   scheduler between dispatches.
   quit: a nullary function returning true when the loop should exit.
         If omitted, runs forever."
    (default quit (fn [] false))
    (g-application-register app nil nil)
    (g-application-activate app)
    (let [ctx (g-main-context-default)]
      (while (not (quit)) (glib-wait ctx))))

  # ── GObject signals ──────────────────────────────────────────────

  (ffi/defbind g-signal-connect-data libgobj "g_signal_connect_data"
               :ulong [:ptr :string :ptr :ptr :ptr :int])

  # ── GObject properties ───────────────────────────────────────────

  (ffi/defbind g-object-set-property-string libgobj "g_object_set"
               :void [:ptr :string :string :ptr])

  # ── WebKit ────────────────────────────────────────────────────────

  (ffi/defbind webkit-web-view-new libwebkit "webkit_web_view_new" :ptr [])
  (ffi/defbind webkit-web-view-load-html libwebkit "webkit_web_view_load_html"
               :void [:ptr :string :ptr])
  (ffi/defbind webkit-web-view-load-uri libwebkit "webkit_web_view_load_uri"
               :void [:ptr :string])
  (ffi/defbind webkit-web-view-evaluate-javascript libwebkit
               "webkit_web_view_evaluate_javascript"
               :void [:ptr :string :ssize :ptr :ptr :ptr :ptr])
  (ffi/defbind webkit-web-view-get-user-content-manager libwebkit
               "webkit_web_view_get_user_content_manager" :ptr [:ptr])
  (ffi/defbind webkit-ucm-register-script-message-handler libwebkit
               "webkit_user_content_manager_register_script_message_handler"
               :int [:ptr :string :string])

  # ── JSC ───────────────────────────────────────────────────────────

  (ffi/defbind jsc-value-to-string libjsc "jsc_value_to_string" :ptr [:ptr])

  # ── Constants ─────────────────────────────────────────────────────

  (def GTK_ORIENTATION_HORIZONTAL 0)
  (def GTK_ORIENTATION_VERTICAL 1)
  (def GTK_WRAP_WORD_CHAR 3)
  (def GTK_STYLE_PROVIDER_PRIORITY_APPLICATION 600)

  # ── Export ────────────────────────────────────────────────────────

  {:libgtk libgtk
   :libglib libglib
   :libgobj libgobj
   :libwebkit libwebkit
   :libjsc libjsc  # gtk init
   :gtk-init gtk-init  # window
   :gtk-window-new gtk-window-new
   :gtk-window-present gtk-window-present
   :gtk-window-close gtk-window-close
   :gtk-window-destroy gtk-window-destroy
   :gtk-window-set-title gtk-window-set-title
   :gtk-window-set-default-size gtk-window-set-default-size
   :gtk-window-fullscreen gtk-window-fullscreen
   :gtk-window-set-child gtk-window-set-child  # widget
   :gtk-widget-show gtk-widget-show
   :gtk-widget-hide gtk-widget-hide
   :gtk-widget-set-visible gtk-widget-set-visible
   :gtk-widget-set-sensitive gtk-widget-set-sensitive
   :gtk-widget-set-size-request gtk-widget-set-size-request
   :gtk-widget-set-hexpand gtk-widget-set-hexpand
   :gtk-widget-set-vexpand gtk-widget-set-vexpand
   :gtk-widget-add-css-class gtk-widget-add-css-class
   :gtk-widget-set-margin-start gtk-widget-set-margin-start
   :gtk-widget-set-margin-end gtk-widget-set-margin-end
   :gtk-widget-set-margin-top gtk-widget-set-margin-top
   :gtk-widget-set-margin-bottom gtk-widget-set-margin-bottom
   :gtk-widget-set-halign gtk-widget-set-halign
   :gtk-widget-set-valign gtk-widget-set-valign
   :gtk-widget-unparent gtk-widget-unparent
   :gtk-widget-queue-draw gtk-widget-queue-draw
   :gtk-widget-add-controller gtk-widget-add-controller  # drawing area
   :gtk-drawing-area-new gtk-drawing-area-new
   :gtk-drawing-area-set-content-width gtk-drawing-area-set-content-width
   :gtk-drawing-area-set-content-height gtk-drawing-area-set-content-height
   :gtk-drawing-area-set-draw-func gtk-drawing-area-set-draw-func  # application
   :gtk-application-new gtk-application-new
   :gtk-application-window-new gtk-application-window-new  # event controllers
   :gtk-gesture-click-new gtk-gesture-click-new
   :gtk-gesture-single-set-button gtk-gesture-single-set-button
   :gtk-gesture-single-get-current-button gtk-gesture-single-get-current-button
   :gtk-event-controller-scroll-new gtk-event-controller-scroll-new
   :gtk-event-controller-key-new gtk-event-controller-key-new  # box
   :gtk-box-new gtk-box-new
   :gtk-box-append gtk-box-append
   :gtk-box-remove gtk-box-remove  # label
   :gtk-label-new gtk-label-new
   :gtk-label-set-text gtk-label-set-text
   :gtk-label-get-text gtk-label-get-text
   :gtk-label-set-xalign gtk-label-set-xalign
   :gtk-label-set-wrap gtk-label-set-wrap  # button
   :gtk-button-new-with-label gtk-button-new-with-label
   :gtk-button-set-label gtk-button-set-label
   :gtk-button-get-label gtk-button-get-label  # toggle
   :gtk-toggle-button-new gtk-toggle-button-new
   :gtk-toggle-button-new-with-label gtk-toggle-button-new-with-label
   :gtk-toggle-button-set-active gtk-toggle-button-set-active
   :gtk-toggle-button-get-active gtk-toggle-button-get-active  # entry
   :gtk-entry-new gtk-entry-new
   :gtk-entry-set-placeholder-text gtk-entry-set-placeholder-text
   :gtk-editable-get-text gtk-editable-get-text
   :gtk-editable-set-text gtk-editable-set-text  # text view
   :gtk-text-view-new gtk-text-view-new
   :gtk-text-view-get-buffer gtk-text-view-get-buffer
   :gtk-text-view-set-wrap-mode gtk-text-view-set-wrap-mode
   :gtk-text-view-set-editable gtk-text-view-set-editable
   :gtk-text-buffer-set-text gtk-text-buffer-set-text
   :gtk-text-buffer-get-start-iter gtk-text-buffer-get-start-iter
   :gtk-text-buffer-get-end-iter gtk-text-buffer-get-end-iter
   :gtk-text-buffer-get-text gtk-text-buffer-get-text  # check
   :gtk-check-button-new-with-label gtk-check-button-new-with-label
   :gtk-check-button-set-active gtk-check-button-set-active
   :gtk-check-button-get-active gtk-check-button-get-active  # switch
   :gtk-switch-new gtk-switch-new
   :gtk-switch-set-active gtk-switch-set-active
   :gtk-switch-get-active gtk-switch-get-active  # scale
   :gtk-scale-new-with-range gtk-scale-new-with-range
   :gtk-range-get-value gtk-range-get-value
   :gtk-range-set-value gtk-range-set-value  # spin button
   :gtk-spin-button-new-with-range gtk-spin-button-new-with-range
   :gtk-spin-button-get-value gtk-spin-button-get-value
   :gtk-spin-button-set-value gtk-spin-button-set-value  # drop down
   :gtk-string-list-new gtk-string-list-new
   :gtk-drop-down-new gtk-drop-down-new
   :gtk-drop-down-get-selected gtk-drop-down-get-selected
   :gtk-drop-down-set-selected gtk-drop-down-set-selected  # search
   :gtk-search-entry-new gtk-search-entry-new  # progress
   :gtk-progress-bar-new gtk-progress-bar-new
   :gtk-progress-bar-set-fraction gtk-progress-bar-set-fraction
   :gtk-progress-bar-get-fraction gtk-progress-bar-get-fraction  # spinner
   :gtk-spinner-new gtk-spinner-new
   :gtk-spinner-start gtk-spinner-start
   :gtk-spinner-stop gtk-spinner-stop  # separator
   :gtk-separator-new gtk-separator-new  # scroll
   :gtk-scrolled-window-new gtk-scrolled-window-new
   :gtk-scrolled-window-set-child gtk-scrolled-window-set-child
   :gtk-scrolled-window-set-min-content-height gtk-scrolled-window-set-min-content-height
   :gtk-scrolled-window-set-min-content-width gtk-scrolled-window-set-min-content-width  # expander
   :gtk-expander-new gtk-expander-new
   :gtk-expander-set-child gtk-expander-set-child
   :gtk-expander-set-expanded gtk-expander-set-expanded  # frame
   :gtk-frame-new gtk-frame-new
   :gtk-frame-set-child gtk-frame-set-child  # grid
   :gtk-grid-new gtk-grid-new
   :gtk-grid-attach gtk-grid-attach
   :gtk-grid-set-row-spacing gtk-grid-set-row-spacing
   :gtk-grid-set-column-spacing gtk-grid-set-column-spacing  # stack
   :gtk-stack-new gtk-stack-new
   :gtk-stack-add-titled gtk-stack-add-titled
   :gtk-stack-set-visible-child-name gtk-stack-set-visible-child-name
   :gtk-stack-get-visible-child-name gtk-stack-get-visible-child-name  # notebook
   :gtk-notebook-new gtk-notebook-new
   :gtk-notebook-append-page gtk-notebook-append-page
   :gtk-notebook-get-current-page gtk-notebook-get-current-page
   :gtk-notebook-set-current-page gtk-notebook-set-current-page  # paned
   :gtk-paned-new gtk-paned-new
   :gtk-paned-set-start-child gtk-paned-set-start-child
   :gtk-paned-set-end-child gtk-paned-set-end-child  # center-box
   :gtk-center-box-new gtk-center-box-new
   :gtk-center-box-set-start-widget gtk-center-box-set-start-widget
   :gtk-center-box-set-center-widget gtk-center-box-set-center-widget
   :gtk-center-box-set-end-widget gtk-center-box-set-end-widget  # overlay
   :gtk-overlay-new gtk-overlay-new
   :gtk-overlay-set-child gtk-overlay-set-child
   :gtk-overlay-add-overlay gtk-overlay-add-overlay  # revealer
   :gtk-revealer-new gtk-revealer-new
   :gtk-revealer-set-child gtk-revealer-set-child
   :gtk-revealer-set-reveal-child gtk-revealer-set-reveal-child
   :gtk-revealer-set-transition-type gtk-revealer-set-transition-type  # image
   :gtk-image-new-from-file gtk-image-new-from-file
   :gtk-image-new-from-icon-name gtk-image-new-from-icon-name
   :gtk-image-set-pixel-size gtk-image-set-pixel-size  # calendar
   :gtk-calendar-new gtk-calendar-new  # css
   :gtk-css-provider-new gtk-css-provider-new
   :gtk-css-provider-load-from-string gtk-css-provider-load-from-string
   :gtk-style-context-add-provider-for-display gtk-style-context-add-provider-for-display
   :gdk-display-get-default gdk-display-get-default  # glib
   :g-main-context-default g-main-context-default
   :g-main-context-iteration g-main-context-iteration
   :g-main-context-pending g-main-context-pending
   :glib-wait glib-wait  # gio / application
   :g-application-register g-application-register
   :g-application-activate g-application-activate
   :g-application-run g-application-run
   :g-application-quit g-application-quit
   :run-app run-app  # gobject
   :g-signal-connect-data g-signal-connect-data  # webkit
   :webkit-web-view-new webkit-web-view-new
   :webkit-web-view-load-html webkit-web-view-load-html
   :webkit-web-view-load-uri webkit-web-view-load-uri
   :webkit-web-view-evaluate-javascript webkit-web-view-evaluate-javascript
   :webkit-web-view-get-user-content-manager webkit-web-view-get-user-content-manager
   :webkit-ucm-register-script-message-handler webkit-ucm-register-script-message-handler
   :jsc-value-to-string jsc-value-to-string  # constants
   :GTK_ORIENTATION_HORIZONTAL GTK_ORIENTATION_HORIZONTAL
   :GTK_ORIENTATION_VERTICAL GTK_ORIENTATION_VERTICAL
   :GTK_WRAP_WORD_CHAR GTK_WRAP_WORD_CHAR
   :GTK_STYLE_PROVIDER_PRIORITY_APPLICATION GTK_STYLE_PROVIDER_PRIORITY_APPLICATION})
# end (fn [])
