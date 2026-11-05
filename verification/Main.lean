import Console
import Evidence
set_option warningAsError true
open Lean Console

def parseEvent (j : Json) : Except String Event := do
  let op : String ← j.getObjValAs? String "op"
  match op with
  | "connected" => pure .connected
  | "arm" => pure .arm
  | "disarm" => pure .disarm
  | "close" => pure .close
  | "keyboard" => return .keyboard (← j.getObjValAs? Nat "modifier") (← j.getObjValAs? (List Nat) "keys")
  | "pointer" =>
    let b ← j.getObjValAs? Nat "buttons"
    let x ← j.getObjValAs? Nat "x"
    let y ← j.getObjValAs? Nat "y"
    let w ← j.getObjValAs? Int "wheel"
    return .pointer b x y w
  | _ => throw "Unsupported operation"

def encodeResult (s : State) (r : Console.Result) : State × Json :=
  let frames := r.payloads.zipIdx |>.map fun ((kind, payload), i) =>
    frame ((s.sequence + i + 1) % 4294967296) kind payload
  let state := {r.state with sequence := (s.sequence + frames.length) % 4294967296}
  (state, Json.mkObj [
    ("accepted", toJson r.accepted), ("connected", toJson state.connected),
    ("armed", toJson state.armed), ("frames", toJson frames)])

def runEvent (s : State) (j : Json) : Except String (State × Json) := do
  let e ← parseEvent j
  return encodeResult s (step s e)

def main (args : List String) : IO UInt32 := do
  if args == ["--verify-evidence"] then
    let input ← IO.getStdin
    let raw ← input.readToEnd
    let result := do
      let j ← Json.parse raw
      let observation : Observation ← fromJson? j
      pure (accepts observation)
    match result with
    | .ok true => IO.println "Evidence contract: PASS"; return 0
    | _ => IO.println "Evidence contract: FAIL"; return 1
  let input ← IO.getStdin
  let output ← IO.getStdout
  let mut state : State := {}
  repeat
    let line ← input.getLine
    if line.isEmpty then break
    let result := do
      if line.length > 4096 then throw "Input too large"
      runEvent state (← Json.parse line)
    let response ← match result with
      | .ok (next, value) => state := next; pure value
      | .error _ =>
          let (next, value) := encodeResult state {step state .disarm with accepted := false}
          state := next
          pure value
    output.putStrLn response.compress
    output.flush
  return 0
