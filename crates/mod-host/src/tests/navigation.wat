(component
  (import "$AUTOMATION" (instance $automation
    (type $c (record (field "forward" bool) (field "backward" bool) (field "left" bool) (field "right" bool)
      (field "jump" bool) (field "sprint" bool) (field "sneak" bool) (field "attack" bool) (field "use-item" bool)))
    (export "controls" (type $controls (eq $c)))
    (export "set-controls" (func (param "value" (option $controls)) (result (result (error string)))))
    (export "select-hotbar" (func (param "slot" u8) (result (result (error string)))))))
  (alias export $automation "set-controls" (func $set-controls))
  (alias export $automation "select-hotbar" (func $select-hotbar))
  (core module $memory-module
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.get 2 i32.add i32.const 1 i32.sub
      i32.const 0 local.get 2 i32.sub i32.and local.tee $old
      local.get 3 i32.add global.set $next local.get $old))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-controls (canon lower (func $set-controls) (memory $memory) (realloc $realloc)))
  (core func $lower-hotbar (canon lower (func $select-hotbar) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "controls" (func $controls (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "host" "hotbar" (func $hotbar (param i32 i32)))
    (func (export "init"))
    (func (export "frame") $FRAME))
  (core instance $host (export "controls" (func $lower-controls)) (export "hotbar" (func $lower-hotbar)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
