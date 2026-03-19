# elle-clap

CLI argument parsing plugin for Elle, powered by [clap](https://docs.rs/clap).

## What it does

`clap/parse` takes a declarative command spec struct and an argv list/array
of strings, then returns a struct of parsed values. No builder objects, no
opaque handles — spec in, plain struct out.

## Building

```bash
cargo build --release -p elle-clap
```

The compiled plugin will be at `target/release/libelle_clap.so`.

## Usage

Load the plugin in Elle:

```lisp
(def plugin (import-file "target/release/libelle_clap.so"))
(def parse-fn (get plugin :parse))
```

### Simple flags

```lisp
(def result
  (parse-fn
    {:name "greet"
     :about "A greeting program"
     :args [{:name "name"    :long "name"    :short "n" :required true}
            {:name "verbose" :long "verbose" :short "v" :action :flag}]}
    ["--name" "world" "-v"]))

(get result :name)     #=> "world"
(get result :verbose)  #=> true
```

### Positional arguments

```lisp
(def result
  (parse-fn
    {:name "cp"
     :args [{:name "src" :required true}
            {:name "dst" :required true}]}
    ["foo.txt" "bar.txt"]))

(get result :src)  #=> "foo.txt"
(get result :dst)  #=> "bar.txt"
```

### Counting flags

```lisp
(def result
  (parse-fn
    {:name "app" :args [{:name "verbose" :short "v" :action :count}]}
    ["-vvv"]))

(get result :verbose)  #=> 3
```

### Append (multi-value)

```lisp
(def result
  (parse-fn
    {:name "app" :args [{:name "include" :short "I" :action :append}]}
    ["-I" "/usr/include" "-I" "/opt/include"]))

(get result :include)  #=> ["/usr/include" "/opt/include"]
```

### Subcommands

```lisp
(def result
  (parse-fn
    {:name "cargo"
     :commands [{:name "build" :args [{:name "release" :long "release" :action :flag}]}
                {:name "test"  :args [{:name "name"}]}]}
    ["build" "--release"]))

(get result :command)                     #=> "build"
(get (get result :command-args) :release) #=> true
```

### Integration with sys/args

```lisp
(def args
  (parse-fn
    {:name "myscript"
     :args [{:name "input"   :required true}
            {:name "output"  :long "output"  :short "o" :default "-"}
            {:name "verbose" :long "verbose" :short "v" :action :flag}]}
    (sys/args)))
```

## Argument spec reference

| Key | Type | Required | Description |
|-----|------|----------|-------------|
| `:name` | string | yes | Argument ID; used as key in result struct |
| `:long` | string | no | Long flag name (`--name`) |
| `:short` | string (1 char) | no | Short flag (`-n`) |
| `:help` | string | no | Help text |
| `:action` | keyword | no | `:set` (default), `:flag`, `:count`, `:append` |
| `:required` | bool | no | Whether the argument is required (default false) |
| `:default` | string | no | Default value (only for `:set` action) |
| `:value` | string | no | Metavar for help display (e.g. `--output <FILE>`) |

An arg with neither `:long` nor `:short` is positional.

## Action types

| Action | Result type | Description |
|--------|-------------|-------------|
| `:set` | string or nil | Takes a value; nil if absent and no default |
| `:flag` | bool | `true` if present, `false` if absent |
| `:count` | int | Number of occurrences (e.g. `-vvv` → 3) |
| `:append` | array of strings | All values collected; `[]` if absent |

## Error handling

clap errors (unknown flag, missing required arg) are signalled as `:error`
with kind `clap-error`. The error message is clap's formatted output, which
includes suggestions and help hints.

```lisp
(let (([ok? result] (protect (parse-fn spec argv))))
  (if ok?
    (process result)
    (display (get result :message) "\n")))
```
