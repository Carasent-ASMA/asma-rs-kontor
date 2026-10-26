// The captured fixture aliases (vite.config.ts): an operator-local JSON file, or the committed empty stub.
declare module "captured:*" {
  const value: unknown;
  export default value;
}
