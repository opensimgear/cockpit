import type { WidgetDefinition, WidgetLayer, WidgetProperty } from "./widget-model.ts";
import { createCapsuleToggle } from './widget-toggle.ts';

const full = { x: 0, y: 0, width: 1, height: 1 };
const colors: WidgetProperty[] = [
  { name: "label", label: "Label", type: "text", default: "CONTROL" },
  { name: "accent", label: "Accent", type: "color", default: "#63dfd0" },
  { name: "foreground", label: "Text", type: "color", default: "#e2f4f3" },
  { name: "background", label: "Background", type: "color", default: "#10212c" },
];
function base(id: string, title: string): WidgetDefinition {
  return {
    formatVersion: 1, id: `osg-${id}`, version: 1, title, size: { width: 200, height: 120 },
    properties: structuredClone(colors), inputs: [], outputs: [], interaction: { kind: "none" }, hitArea: { ...full }, state: {},
    layers: [
      { id: "bezel", label: "Bezel", kind: "rectangle", bounds: { ...full }, fill: { property: "background" }, stroke: { property: "accent" }, strokeWidth: 2, radius: 10, states: { pressed: { fill: "#27505c", strokeWidth: 3 }, active: { strokeWidth: 4 }, warning: { stroke: "#ff765e" }, disabled: { opacity: .35 }, unknown: { opacity: .55 }, stale: { opacity: .45 }, disconnected: { opacity: .3 } } },
      { id: "label", label: "Label", kind: "text", bounds: { x: .08, y: .33, width: .84, height: .34 }, text: { property: "label" }, font: "sans", fontSize: 22, align: "center", fill: { property: "foreground" } },
    ],
  };
}
/** Original portable artwork built from editable layers, with no external assets. */
export function createWidgetCatalog(): WidgetDefinition[] {
  const button = base("illuminated-button", "Illuminated button");
  button.properties[0].default = "ENGINE";
  button.inputs = [{ name: "press", label: "Press", kind: "button" }];
  button.outputs = [{ name: "active", label: "Confirmed active", kind: "indicator" }];
  button.interaction = { kind: "momentary", port: "press" }; button.state = { confirmedPort: "active" };
  button.layers.push({ id: "status", label: "Confirmed status", kind: "indicator", bounds: { x: .45, y: .78, width: .1, height: .1 }, outputPort: "active", fill: { property: "accent" } });

  const toggle = structuredClone(button); toggle.id = "osg-toggle"; toggle.title = "Illuminated toggle";
  toggle.properties[0].default = "POWER"; toggle.interaction = { kind: "toggle", port: "press" };

  const selector = base("mode-selector", "Mode selector"); selector.properties[0].default = "MODE";
  selector.inputs = [{ name: "selection", label: "Selected mode", kind: "axis", axisMode: "unipolar", fallback: 0 }];
  selector.interaction = { kind: "selector", port: "selection", options: [{ label: "Off", value: 0 }, { label: "Standby", value: .5 }, { label: "On", value: 1 }] };

  const indicator = base("annunciator", "Annunciator"); indicator.properties[0].default = "STATUS";
  indicator.outputs = [{ name: "active", label: "Confirmed active", kind: "indicator" }]; indicator.state = { confirmedPort: "active" };
  indicator.layers.push({ id: "status", label: "Status light", kind: "indicator", bounds: { x: .08, y: .08, width: .12, height: .14 }, outputPort: "active", fill: { property: "accent" } });

  const numeric = base("numeric-readout", "Numeric instrument"); numeric.properties[0].default = "LEVEL";
  numeric.outputs = [{ name: "value", label: "Value", kind: "gauge", unit: "%", range: { min: 0, max: 100 } }];
  numeric.layers[1].bounds = { x: .08, y: .08, width: .84, height: .25 };
  const readout: WidgetLayer = { id: "value", label: "Live value", kind: "text", bounds: { x: .08, y: .4, width: .84, height: .4 }, text: "—", outputPort: "value", font: "mono", fontSize: 36, align: "center", fill: { property: "foreground" }, precision: 1, unit: "%" };
  numeric.layers.push(readout);

  const bar = structuredClone(numeric); bar.id = "osg-linear-gauge"; bar.title = "Linear gauge";
  bar.properties.push({ name: "minimum", label: "Minimum", type: "number", default: 0, min: -1e9, max: 1e9 }, { name: "maximum", label: "Maximum", type: "number", default: 100, min: -1e9, max: 1e9 });
  bar.layers[2].bounds = { x: .08, y: .35, width: .84, height: .3 };
  bar.layers.push({ id: "gauge", label: "Gauge", kind: "gauge", bounds: { x: .08, y: .75, width: .84, height: .12 }, outputPort: "value", min: { property: "minimum" }, max: { property: "maximum" }, orientation: "horizontal", fill: { property: "accent" }, stroke: "#30434d", strokeWidth: 2 });

  const radial = structuredClone(bar); radial.id = "osg-radial-gauge"; radial.title = "Radial gauge"; radial.size = { width: 200, height: 200 };
  radial.layers[1].bounds = { x: .15, y: .68, width: .7, height: .16 };
  radial.layers[2].bounds = { x: .2, y: .4, width: .6, height: .22 };
  radial.layers[3] = { ...radial.layers[3], bounds: { x: .12, y: .1, width: .76, height: .76 }, orientation: "radial", strokeWidth: 8 } as WidgetLayer;
  // Arc behind its labels: layer ordering is part of the editable artwork.
  radial.layers = [radial.layers[0], radial.layers[3], radial.layers[1], radial.layers[2]];

  const frame = base("frame", "Section frame"); frame.properties[0].default = "SYSTEMS";
  frame.layers[0].opacity = .65; frame.layers[1].bounds = { x: .08, y: .05, width: .84, height: .2 };
  const divider = base("divider", "Divider"); divider.layers = [{ id: "line", label: "Divider", kind: "line", bounds: { x: .02, y: .49, width: .96, height: .02 }, stroke: { property: "accent" }, strokeWidth: 2 }];

  const navigation = base("page-link", "Page navigation"); navigation.properties[0].default = "NEXT";
  navigation.properties.push({ name: "page", label: "Destination page", type: "text", default: "" }); navigation.interaction = { kind: "navigate", pageProperty: "page" };

  const text = base("text-instrument", "Text instrument"); text.properties[0].default = "MESSAGE";
  text.outputs = [{ name: "value", label: "Text", kind: "text" }];
  text.layers[1].bounds = { x: .08, y: .08, width: .84, height: .25 };
  text.layers.push({ id: "value", label: "Live text", kind: "text", bounds: { x: .08, y: .4, width: .84, height: .4 }, text: "—", outputPort: "value", font: "mono", fontSize: 24, align: "center", fill: { property: "foreground" } });

  const icon = base("power-icon", "Power icon"); icon.properties[0].default = "POWER"; icon.size = { width: 120, height: 120 };
  icon.outputs = [{ name: "active", label: "Confirmed active", kind: "indicator" }]; icon.state = { confirmedPort: "active" };
  icon.layers[1].bounds = { x: .08, y: .75, width: .84, height: .2 };
  icon.layers.push(
    { id: "ring", label: "Power ring", kind: "ellipse", bounds: { x: .25, y: .15, width: .5, height: .5 }, fill: { property: "background" }, stroke: "#42515a", strokeWidth: 6, states: { active: { stroke: { property: "accent" } } } },
    { id: "gap", label: "Ring opening", kind: "rectangle", bounds: { x: .44, y: .1, width: .12, height: .2 }, fill: { property: "background" } },
    { id: "stem", label: "Power stem", kind: "rectangle", bounds: { x: .48, y: .12, width: .04, height: .29 }, fill: "#42515a", states: { active: { fill: { property: "accent" } } } },
  );
  return [button, toggle, selector, indicator, numeric, bar, radial, frame, divider, navigation, text, icon, createCapsuleToggle(false), createCapsuleToggle(true)];
}

/** Saved designs retain their exact versions; new built-ins remain discoverable after updates. */
export function includeBuiltinWidgets(definitions: WidgetDefinition[]): WidgetDefinition[] {
  const retained = new Set(definitions.map(definition => definition.id));
  return [...definitions, ...createWidgetCatalog().filter(definition => !retained.has(definition.id))];
}
