import { createSignal } from "solid-js";

export function List() {
  const [items, setItems] = createSignal(["alpha", "beta"]);
  return (
    <ul>
      {items().map((item) => <li>{item}</li>)}
    </ul>
  );
}
