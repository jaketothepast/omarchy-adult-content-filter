function requestLaunch(running) {
  return { start: !running, focus: !!running }
}

function requestStop(running) {
  return { stop: !!running, stopping: !!running }
}

function finishSupervisor(exitCode, stopping) {
  var code = Number(exitCode)
  var expectedStop = !!stopping && (code === 0 || code === 15 || code === 143)
  return {
    running: false,
    stopping: false,
    exitCode: code,
    error: code === 0 || expectedStop
      ? ""
      : "Managed browser supervisor exited with status " + code
  }
}

if (typeof module !== "undefined") {
  module.exports = {
    requestLaunch: requestLaunch,
    requestStop: requestStop,
    finishSupervisor: finishSupervisor
  }
}
