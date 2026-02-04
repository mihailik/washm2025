(module
  (import "env" "sleep" (func $sleep))
  (import "env" "tunnel" (func $tunnel (param $x i32) (result i32)))
  (memory 1 2)
  (global $temp (mut i32) (i32.const 0))
  (func $factorial-loop (export "factorial-loop") (param $x i32) (result i32)
    (local $i i32)
    (local $ret i32)
    (local.set $ret (i32.const 1))
    (local.set $i (i32.const 2))
    (loop $l
      (if
        (i32.gt_u
          (local.get $i)
          (local.get $x)
        )
        (then
          (return (local.get $ret))
        )
      )
      (local.set $ret
        (i32.mul
          (local.get $ret)
          (local.get $i)
        )
      )
      (call $sleep)
      (local.set $i
        (i32.add
          (local.get $i)
          (i32.const 1)
        )
      )
      (br $l)
    )
  )
)
