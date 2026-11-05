import { createSignal } from "solid-js";

export function Swatch() {
  const [color, setColor] = createSignal("crimson");
  return <div style={{ color: color(), "font-size": "14px" }}>swatch</div>;
}
