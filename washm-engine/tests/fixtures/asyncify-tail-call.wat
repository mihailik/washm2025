(module
  (import "env" "sleep" (func $sleep))
  (import "asyncify" "start_unwind" (func $start_unwind (param i32)))
  (import "asyncify" "stop_unwind" (func $stop_unwind))
  (import "asyncify" "start_rewind" (func $start_rewind (param i32)))
  (import "asyncify" "stop_rewind" (func $stop_rewind))

  (func $tail_sleep (export "tail_sleep")
    (return_call $sleep)
  )

  (func $normal_call (export "normal_call")
    (call $tail_sleep)
  )
)
