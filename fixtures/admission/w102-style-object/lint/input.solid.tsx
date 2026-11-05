export function Badge() {
  const [color, setColor] = createSignal("red");
  return <div style={{ color: color() }}>badge</div>;
}
