// tree-sitter query files are bundled as strings (esbuild `.scm: text` loader).
declare module "*.scm" {
  const content: string;
  export default content;
}
