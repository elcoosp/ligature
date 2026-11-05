export function Metric() {
  const [n, setN] = createSignal(1);
  return <p>{n()}</p>;
}
