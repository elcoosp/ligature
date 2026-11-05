export function Sum() {
  const [a, setA] = createSignal(2);
  const [b, setB] = createSignal(3);
  const total = createMemo(() => a() + b());
  return <output>{total()}</output>;
}
