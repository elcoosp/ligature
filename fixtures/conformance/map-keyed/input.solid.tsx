import { createSignal } from "solid-js";

export function Rows() {
  const [items, setItems] = createSignal(["alpha", "beta"]);
  return (
    <ul>
      {items().map((item) => (
        <li key={item}>{item}</li>
      ))}
    </ul>
  );
}
