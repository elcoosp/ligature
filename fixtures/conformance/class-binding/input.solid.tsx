export function Badge() {
  const [active, setActive] = createSignal(false);
  return <span class={active() ? "badge on" : "badge"}>B</span>;
}
