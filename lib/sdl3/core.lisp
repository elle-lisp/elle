(elle/epoch 14)
# audited: 2026-09-30
## What every SDL3 submodule shares: the error checks on a C return, and the rectangle types.
## lib/overview.md

(fn [&named libsdl]

  (ffi/defbind sdl-get-error libsdl "SDL_GetError" :ptr [])

  (def frect-type (ffi/struct @[:float :float :float :float]))
  (def irect-type (ffi/struct @[:int :int :int :int]))

  (defn null? [ptr]
    (= (ptr/to-int ptr) 0))

  (defn error-string []
    "Return the current SDL error string."
    (ffi/string (sdl-get-error)))

  (defn sdl-error [name]
    "Raise an SDL error with context from SDL_GetError."
    (error {:error :sdl-error
            :fn name
            :message (concat name ": " (error-string))}))

  (defn check-bool [ok name]
    "Check an SDL bool return; error if false."
    (when (not ok) (sdl-error name))
    true)

  (defn check-ptr [ptr name]
    "Check an SDL pointer return; error if NULL."
    (when (null? ptr) (sdl-error name))
    ptr)

  (defn write-frect [buf x y w h]
    "Write an SDL_FRect into buf."
    (ffi/write buf :float x)
    (ffi/write (ptr/add buf 4) :float y)
    (ffi/write (ptr/add buf 8) :float w)
    (ffi/write (ptr/add buf 12) :float h))

  {:frect-type frect-type
   :irect-type irect-type
   :null? null?
   :error-string error-string
   :sdl-error sdl-error
   :check-bool check-bool
   :check-ptr check-ptr
   :write-frect write-frect})
