export function Metric() {
  const [n, setN] = createSignal(1);
  return <p>{native("n as u32")}</p>;
}
