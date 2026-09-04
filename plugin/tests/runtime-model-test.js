const assert = require("node:assert/strict")
const model = require("../RuntimeModel.js")

assert.deepEqual(model.requestLaunch(false), { start: true, focus: false })
assert.deepEqual(model.requestLaunch(true), { start: false, focus: true })

assert.deepEqual(model.requestStop(false), { stop: false, stopping: false })
assert.deepEqual(model.requestStop(true), { stop: true, stopping: true })

assert.deepEqual(model.finishSupervisor(0, false), {
  running: false,
  stopping: false,
  exitCode: 0,
  error: ""
})
assert.deepEqual(model.finishSupervisor(143, true), {
  running: false,
  stopping: false,
  exitCode: 143,
  error: ""
})
assert.deepEqual(model.finishSupervisor(70, false), {
  running: false,
  stopping: false,
  exitCode: 70,
  error: "Managed browser supervisor exited with status 70"
})

console.log("ok - plugin supervisor lifecycle model")
