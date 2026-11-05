import Lean
set_option warningAsError true

/- The live gateway uses this transition function. These are guarantees about
   permitted reports, not claims about USB delivery or the physical desktop. -/
namespace Console

structure State where
  connected : Bool := false
  armed : Bool := false
  sequence : Nat := 0
  pointerX : Nat := 32768
  pointerY : Nat := 32768
  pointerUsed : Bool := false
  deriving Repr, DecidableEq

inductive Event where
  | connected
  | arm
  | disarm
  | close
  | keyboard (modifier : Nat) (keys : List Nat)
  | pointer (buttons x y : Nat) (wheel : Int)
  deriving Repr

structure Result where
  state : State
  payloads : List (Nat × List Nat) := []
  accepted : Bool := true
  deriving Repr

def bytes16 (n : Nat) : List Nat := [n % 256, n / 256 % 256]

def neutral (s : State) : List (Nat × List Nat) :=
  [(1, [0,0,0,0,0,0,0,0])] ++
  if s.pointerUsed then [(3, [0] ++ bytes16 s.pointerX ++ bytes16 s.pointerY ++ [0,0])] else []

def validKeys (modifier : Nat) (keys : List Nat) : Bool :=
  modifier < 256 && keys.length ≤ 6 && keys.all (fun k => 4 ≤ k && k ≤ 231)

def validPointer (buttons x y : Nat) (wheel : Int) : Bool :=
  buttons ≤ 31 && x ≤ 65535 && y ≤ 65535 && -32768 ≤ wheel && wheel ≤ 32767

def step (s : State) (e : Event) : Result :=
  match e with
  | .connected => ⟨{connected := true}, [], true⟩
  | .arm => ⟨{s with armed := s.connected}, [], s.connected⟩
  | .disarm => ⟨{s with armed := false}, if s.connected && s.armed then neutral s else [], true⟩
  | .close => ⟨{s with connected := false, armed := false}, if s.connected && s.armed then neutral s else [], true⟩
  | .keyboard modifier keys =>
    if s.connected && s.armed && validKeys modifier keys then
      ⟨s, [(1, [modifier, 0] ++ keys ++ List.replicate (6 - keys.length) 0)], true⟩
    else ⟨s, [], false⟩
  | .pointer buttons x y wheel =>
    if s.connected && s.armed && validPointer buttons x y wheel then
      ⟨{s with pointerX := x, pointerY := y, pointerUsed := true}, [(3, [buttons] ++ bytes16 x ++ bytes16 y ++ bytes16 (wheel % 65536).toNat)], true⟩
    else ⟨s, [], false⟩

theorem keyboard_requires_arm (s : State) (m : Nat) (ks : List Nat)
    (h : (step s (.keyboard m ks)).accepted = true) :
    s.connected = true ∧ s.armed = true ∧ validKeys m ks = true := by
  simp only [step] at h
  split at h
  · rename_i guard
    simp only [Bool.and_eq_true] at guard
    exact ⟨guard.1.1, guard.1.2, guard.2⟩
  · contradiction

theorem pointer_requires_arm (s : State) (b x y : Nat) (w : Int)
    (h : (step s (.pointer b x y w)).accepted = true) :
    s.connected = true ∧ s.armed = true ∧ validPointer b x y w = true := by
  simp only [step] at h
  split at h
  · rename_i guard
    simp only [Bool.and_eq_true] at guard
    exact ⟨guard.1.1, guard.1.2, guard.2⟩
  · contradiction

theorem reconnect_disarmed (s : State) : (step s .connected).state.armed = false := rfl
theorem close_disarmed (s : State) : (step s .close).state.armed = false := rfl
theorem close_disconnected (s : State) : (step s .close).state.connected = false := rfl
theorem disarm_clears (s : State) : (step s .disarm).state.armed = false := rfl
theorem close_releases (s : State) (hc : s.connected = true) (ha : s.armed = true) :
    (step s .close).payloads = neutral s := by simp [step, hc, ha]
theorem disarm_releases (s : State) (hc : s.connected = true) (ha : s.armed = true) :
    (step s .disarm).payloads = neutral s := by simp [step, hc, ha]
theorem rejected_keyboard_has_no_payload (s : State) (m : Nat) (ks : List Nat)
    (h : (step s (.keyboard m ks)).accepted = false) :
    (step s (.keyboard m ks)).payloads = [] := by
  by_cases hc : s.connected = true <;>
    by_cases ha : s.armed = true <;>
      by_cases hk : validKeys m ks = true <;>
        simp_all [step]

def bytes32 (n : Nat) : List Nat := bytes16 n ++ bytes16 (n / 65536)
def frame (sequence kind : Nat) (payload : List Nat) : List Nat :=
  [3,1,(if kind = 1 then 1 else 2),kind,0] ++ bytes32 sequence ++ [0,0,0,0] ++ payload

theorem frame_channel (seq kind : Nat) (payload : List Nat) :
    (frame seq kind payload).head? = some 3 := by simp [frame]

end Console
