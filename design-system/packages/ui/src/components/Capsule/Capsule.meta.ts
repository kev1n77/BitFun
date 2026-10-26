import type { ComponentMeta } from "../../registry.types";

export const capsuleMeta = {
  category: "action",
  description: "A capsule composition with a circular glass action that overlays the label or occupies reserved space inside its Button surface. Reserved content is centered at rest and shifts aside when the action appears, without changing layout; touch keeps the action available.",
  maturity: "stable",
  name: "Capsule",
  props: [
    { name: "children", type: "ReactNode" },
    { name: "action", type: "ReactNode" },
    { name: "actionPlacement", type: "overlay | reserved", defaultValue: "overlay" },
    { name: "size", type: "xs | sm | md | lg", defaultValue: "sm" },
  ],
  states: ["default", "hover", "focus-visible", "disabled"],
  tokens: ["control.button.xsHeight", "control.height.sm", "control.height.md", "control.height.lg", "control.iconButton.xsSize", "color.surface.raised", "color.border.subtle", "effect.blur.subtle", "radius.pill"],
} as const satisfies ComponentMeta;
