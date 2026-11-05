export function Rich() {
  const [h, setH] = createSignal("<b>x</b>");
  return <div innerHTML={h()} />;
}
