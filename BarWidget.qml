import QtQuick
import qs.Ui

BarWidget {
  id: root
  moduleName: "io.github.jaketothepast.adult-content-filter"

  property string pluginId: "io.github.jaketothepast.adult-content-filter"
  readonly property var service: root.bar && root.bar.shell
    ? root.bar.shell.serviceFor(root.pluginId)
    : null

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: "󰒃"
    active: root.service ? root.service.running : false
    tooltipText: !root.service
      ? "Adult filter unavailable"
      : (root.service.running
          ? "Adult filter browser running · right-click to stop"
          : (root.service.lastError || "Launch adult filter browser"))

    onPressed: function(mouseButton) {
      if (!root.service) return
      if (mouseButton === Qt.RightButton) root.service.stop()
      else root.service.launch()
    }
  }
}
