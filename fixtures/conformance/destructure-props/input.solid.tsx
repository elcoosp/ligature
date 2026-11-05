export function Greeting({ name, greeting }: { name: string; greeting: string }) {
  return <h1>{greeting} {name}</h1>;
}
