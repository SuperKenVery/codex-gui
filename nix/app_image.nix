{
  bundlers,
  codexGui,
  system,
}:
bundlers.bundlers.${system}.toAppImage codexGui
