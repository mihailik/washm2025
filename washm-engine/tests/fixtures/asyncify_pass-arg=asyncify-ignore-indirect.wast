(module
  (import "env" "imp" (func $imp (param i32) (result i32)))
  (table 1 funcref)
  (elem (i32.const 0) $imp)
  (func $i (export "i") (param $x i32) (result i32)
    (call_indirect (type 0) (local.get $x) (local.get $x))
  )
  (type (func (param i32) (result i32)))
)
