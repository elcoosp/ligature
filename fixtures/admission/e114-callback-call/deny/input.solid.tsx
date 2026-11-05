export function List({ items }) {
  return <p>{items.filter(i => i.active).length}</p>;
}
