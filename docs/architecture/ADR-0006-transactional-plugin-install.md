# ADR-0006: Instalação transacional de pacotes externos

- Status: aceito
- Data: 2026-09-02

## Contexto

Um pacote de plugin é entrada não confiável. Extrair diretamente no diretório
ativo permitiria deixar instalações parciais e ampliaria o impacto de path
traversal, links, arquivos especiais e bombas de descompressão. Validar apenas
o `plugin.json` também não protege o restante do asset.

## Decisão

O primeiro formato externo é `.tar.zst`. A instalação local é dividida em duas
fases. `stage` verifica o arquivo regular e seu limite comprimido, calcula o
SHA-256 e o compara com um descritor externo, extrai em diretório privado e
temporário e devolve uma revisão tipada. `install` exige aprovação exata das
permissões e move o conteúdo validado para `<root>/packages/<id>/<version>`.

A extração aceita somente diretórios e arquivos regulares com paths relativos,
normais e UTF-8. Links, tipos especiais, duplicatas, paths longos e traversal
são recusados. Há limites independentes para o fluxo descomprimido, quantidade
de entradas, arquivo individual e soma do conteúdo. Modos vindos do TAR são
ignorados; diretórios ficam privados e arquivos, inclusive o entrypoint, ficam
sem bit de execução.

O manifesto é validado novamente como origem externa, o asset precisa coincidir
com o descritor e somente runtime `process` é aceito. O entrypoint declarado
precisa existir como arquivo regular. Falha ou abandono remove o staging. A
versão final nunca é substituída e a instalação nasce desabilitada.

## Consequências

- nenhum conteúdo parcial é publicado no destino final;
- aprovação de permissões é uma fronteira explícita entre revisão e instalação;
- selecionar um arquivo local não confere autenticidade: o descritor esperado
  ainda precisa vir de um catálogo ou canal confiável;
- download, assinatura, habilitação e execução são etapas separadas e
  permanecem fora deste incremento; a descoberta posterior é definida pela
  ADR-0007;
- antes de habilitar um runtime externo, o host deverá materializar permissões
  executáveis e aplicar o sandbox definido pela política.
