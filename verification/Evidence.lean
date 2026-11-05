import Lean
set_option warningAsError true
open Lean
namespace Console

/- An explicit contract for observed evidence. It cannot attest the honesty of
   measurements, source provenance, the compiler, or physical hardware. -/
structure Observation where
  sourceDigest : String
  currentDigest : String
  device : String
  expectedDevice : String
  targetActiveBefore : Bool
  targetActiveAfter : Bool
  heldKeysAfter : List Nat
  memoryBefore : Bool
  runningBefore : Bool
  memoryAfter : Bool
  runningAfter : Bool
  framesBefore : Nat
  framesAfter : Nat
  width : Nat
  height : Nat
  red : List Nat
  green : List Nat
  blue : List Nat
  expectedText : String
  receivedText : String
  keyPresses : Nat
  keyReleases : Nat
  clicksBefore : Nat
  clicksAfter : Nat
  mouseX : Nat
  mouseY : Nat
  expectedX : Nat
  expectedY : Nat
  closed : Bool
  reconnectFrames : Nat
  reconnectArmed : Bool
  deriving FromJson, ToJson

def near (a b : Nat) : Bool := a ≤ b + 4 && b ≤ a + 4
def redPixel (p : List Nat) : Bool := match p with
  | [r,g,b] => 150 < r && r ≤ 255 && g < 100 && b < 100
  | _ => false
def greenPixel (p : List Nat) : Bool := match p with
  | [r,g,b] => r < 100 && 150 < g && g ≤ 255 && b < 100
  | _ => false
def bluePixel (p : List Nat) : Bool := match p with
  | [r,g,b] => r < 100 && g < 120 && 150 < b && b ≤ 255
  | _ => false
def sourceValid (o : Observation) : Bool :=
  o.sourceDigest.length = 64 && o.sourceDigest == o.currentDigest &&
  !o.expectedDevice.isEmpty && o.device == o.expectedDevice
def memorySafe (o : Observation) : Bool :=
  !o.memoryBefore && !o.runningBefore && !o.memoryAfter && !o.runningAfter
def videoValid (o : Observation) : Bool :=
  0 < o.framesBefore && o.framesBefore < o.framesAfter && 0 < o.width && 0 < o.height &&
  redPixel o.red && greenPixel o.green && bluePixel o.blue
def keyboardValid (o : Observation) : Bool :=
  !o.expectedText.isEmpty && o.expectedText == o.receivedText &&
  o.keyPresses = o.expectedText.length && o.keyReleases = o.keyPresses &&
  o.targetActiveBefore && o.targetActiveAfter && o.heldKeysAfter.isEmpty
def mouseValid (o : Observation) : Bool :=
  o.clicksAfter = o.clicksBefore + 1 && near o.mouseX o.expectedX && near o.mouseY o.expectedY
def recoveryValid (o : Observation) : Bool :=
  o.closed && 0 < o.reconnectFrames && !o.reconnectArmed
def accepts (o : Observation) : Bool :=
  sourceValid o && memorySafe o && videoValid o && keyboardValid o && mouseValid o && recoveryValid o

theorem accepted_has_all_observations (o : Observation) (h : accepts o = true) :
    sourceValid o = true ∧ memorySafe o = true ∧ videoValid o = true ∧
    keyboardValid o = true ∧ mouseValid o = true ∧ recoveryValid o = true := by
  simpa only [accepts, Bool.and_eq_true, and_assoc] using h

theorem accepted_requires_recovery (o : Observation) (h : accepts o = true) :
    o.closed = true ∧ 0 < o.reconnectFrames ∧ o.reconnectArmed = false := by
  have hr := (accepted_has_all_observations o h).2.2.2.2.2
  simpa [recoveryValid, and_assoc] using hr
end Console
