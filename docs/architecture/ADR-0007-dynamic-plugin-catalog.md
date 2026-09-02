# ADR-0007: Catálogo dinâmico e integridade após instalação

- Status: aceito
- Data: 2026-09-02

## Contexto

Mover um pacote validado para o armazenamento não basta para disponibilizá-lo
com segurança depois de reiniciar o IDE. O catálogo precisa reconstruir o
estado a partir do disco sem confiar em nomes de diretório ou em um manifesto
que possa ter sido alterado após a instalação. Atualizações também não podem
herdar habilitação ou concessões de outra versão silenciosamente.

## Decisão

Cada instalação externa recebe um recibo host-managed com versão do formato,
identidade e versão do plugin, descritor do asset e SHA-256 determinístico da
árvore extraída. O hash inclui paths, tipos, modos, tamanhos e conteúdo de todos
os arquivos, exceto o próprio recibo.

Na inicialização e em cada reload, o núcleo percorre somente
`plugins/packages/<id>/<version>`, recusa layouts, links e tipos inesperados,
valida o recibo, recalcula o hash, valida novamente o `plugin.json` e o
entrypoint e exige que identidade e versão correspondam aos diretórios. Uma
falha remove todos os plugins externos do catálogo em memória e do estado de
autoridade; os plugins embutidos continuam disponíveis.

O catálogo combina os manifests embutidos com a versão SemVer externa mais
recente de cada ID. Um pacote externo não pode substituir um ID embutido. O
estado local v4 persiste `id → versão`, habilitação e concessões separadamente.
Instalações novas aparecem instaladas, mas desabilitadas. A aprovação exata
pode ser registrada após a instalação sem habilitar o plugin. Qualquer mudança
de versão remove habilitação e concessões até nova revisão.

## Consequências

- reiniciar o IDE não confia cegamente no conteúdo instalado;
- corrupção de qualquer versão externa falha fechada para o catálogo externo;
- versões antigas podem permanecer armazenadas, mas somente a mais recente é
  publicada;
- o estado v3 é migrado usando as versões do catálogo validado;
- o recibo detecta corrupção e adulteração acidental, mas não substitui uma
  assinatura de publisher nem autentica um atacante com acesso ao estado local;
- remoção física, UI de revisão, downloads e assinatura permanecem etapas
  posteriores.
