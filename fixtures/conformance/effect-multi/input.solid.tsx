import { createSignal } from "solid-js";

export function Sync() {
  const [tick, setTick] = createSignal(0);
  const [log, setLog] = createSignal("");
  const [count, setCount] = createSignal(0);
  createEffect(() => {
    setLog(tick().toString());
    setCount(tick() * 2);
  });
  return <em>{log()}</em>;
}
