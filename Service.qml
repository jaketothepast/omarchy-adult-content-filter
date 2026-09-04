import QtQuick
import Quickshell.Io
import "plugin/RuntimeModel.js" as RuntimeModel

Item {
  id: root

  property var manifest: null
  property var shell: null
  property bool stopping: false
  property int lastExitCode: 0
  property string lastError: ""

  readonly property string pluginDir: manifest && manifest.__sourceDir
    ? String(manifest.__sourceDir)
    : ""
  readonly property string pluginId: manifest && manifest.id
    ? String(manifest.id)
    : "io.github.jaketothepast.adult-content-filter"
  readonly property bool running: supervisorProcess.running

  function launch() {
    var transition = RuntimeModel.requestLaunch(supervisorProcess.running)
    if (transition.focus) return focus()
    if (!transition.start) return false
    if (root.pluginDir === "") {
      root.lastError = "Plugin source directory is unavailable"
      return false
    }

    root.stopping = false
    root.lastError = ""
    supervisorProcess.command = [root.pluginDir + "/bin/omarchy-adult-content-filter"]
    supervisorProcess.running = true
    return true
  }

  function stop() {
    var transition = RuntimeModel.requestStop(supervisorProcess.running)
    root.stopping = transition.stopping
    if (transition.stop) supervisorProcess.running = false
    return transition.stop
  }

  function focus() {
    root.lastError = "Managed browser is already running"
    return false
  }

  IpcHandler {
    target: root.pluginId

    function launch(): string {
      return root.launch() ? "started" : (root.running ? "running" : "error")
    }

    function stop(): string {
      return root.stop() ? "stopping" : "stopped"
    }

    function status(): string {
      return JSON.stringify({
        running: root.running,
        stopping: root.stopping,
        lastExitCode: root.lastExitCode,
        lastError: root.lastError
      })
    }
  }

  Process {
    id: supervisorProcess
    running: false
    command: []

    onExited: function(exitCode) {
      var transition = RuntimeModel.finishSupervisor(exitCode, root.stopping)
      root.stopping = transition.stopping
      root.lastExitCode = transition.exitCode
      root.lastError = transition.error
    }
  }
}
