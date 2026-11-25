;; A contract for external contract calls (NEP-667), built into
;; external_call.wasm with `wat`.
;;
;; `__external_call` logs "checking", then refuses a payload starting with
;; `r` by panicking with "refused". It accepts any other payload: it stores
;; the payload under "last", sends 1 yoctoNEAR to itself, logs "accepted"
;; and returns the JSON string "ok".
;;
;; `noop` does nothing, so a signed transaction can call the contract.
(module
  (import "env" "input" (func $input (param i64)))
  (import "env" "read_register" (func $read_register (param i64 i64)))
  (import "env" "register_len" (func $register_len (param i64) (result i64)))
  (import "env" "log_utf8" (func $log_utf8 (param i64 i64)))
  (import "env" "panic_utf8" (func $panic_utf8 (param i64 i64)))
  (import "env" "value_return" (func $value_return (param i64 i64)))
  (import "env" "storage_write"
    (func $storage_write (param i64 i64 i64 i64 i64) (result i64)))
  (import "env" "current_account_id" (func $current_account_id (param i64)))
  (import "env" "promise_batch_create"
    (func $promise_batch_create (param i64 i64) (result i64)))
  (import "env" "promise_batch_action_transfer"
    (func $promise_batch_action_transfer (param i64 i64)))
  (memory (export "memory") 1)
  (data (i32.const 1024) "accepted")
  (data (i32.const 1040) "refused")
  (data (i32.const 1056) "checking")
  (data (i32.const 1072) "last")
  (data (i32.const 1088) "\"ok\"")
  ;; 1 yoctoNEAR as a little-endian u128.
  (data (i32.const 1104) "\01\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00")
  (func (export "__external_call")
    (call $log_utf8 (i64.const 8) (i64.const 1056))
    ;; The payload goes to 8192, past the data above.
    (call $input (i64.const 0))
    (call $read_register (i64.const 0) (i64.const 8192))
    (if (i64.gt_u (call $register_len (i64.const 0)) (i64.const 0))
      (then
        (if (i32.eq (i32.load8_u (i32.const 8192)) (i32.const 0x72))
          (then (call $panic_utf8 (i64.const 7) (i64.const 1040))))))
    (drop (call $storage_write
      (i64.const 4) (i64.const 1072)
      (call $register_len (i64.const 0)) (i64.const 8192)
      (i64.const 1)))
    (call $current_account_id (i64.const 2))
    (call $read_register (i64.const 2) (i64.const 4096))
    (call $promise_batch_action_transfer
      (call $promise_batch_create (call $register_len (i64.const 2)) (i64.const 4096))
      (i64.const 1104))
    (call $log_utf8 (i64.const 8) (i64.const 1024))
    (call $value_return (i64.const 4) (i64.const 1088)))
  (func (export "noop"))
)
