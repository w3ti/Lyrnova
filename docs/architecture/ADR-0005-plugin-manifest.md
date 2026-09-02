# ADR-0005: Manifestos estritos e concessões separadas para plugins

- Status: aceito
- Data: 2026-09-02

## Contexto

O catálogo inicial descrevia capabilities e permissões como strings mantidas
diretamente no binário. Instalar ou habilitar um item persistia apenas seu ID.
Esse formato não representava compatibilidade, runtime, origem, integridade nem
quais permissões haviam sido efetivamente aceitas.

## Decisão

Adotar `plugin.json` versionado e validado pelo núcleo. Capabilities,
permissões, tipos de plugin, runtime e origem usam enums fechados. Manifests
embutidos passam pelo mesmo parser destinado aos pacotes futuros.

As concessões ficam separadas do manifesto no estado local. Instalação exige
aprovação exata do conjunto declarado; permissões adicionais ou ausentes
falham. Uma mudança no conjunto solicitado desabilita o plugin até nova
revisão. Declaração, concessão, instalação e habilitação são verificadas antes
de uma ação sensível.

Pacotes de GitHub Release precisam de SHA-256 válido em um descritor externo ao
arquivo. O manifesto mantém a identidade do asset, mas não incorpora o hash do
próprio pacote, o que seria circular. A origem também é um parâmetro do parser,
impedindo que um documento externo se declare embutido.

## Consequências

- manifests desconhecidos ou incompatíveis falham fechados;
- IDs duplicados impedem o carregamento do catálogo;
- entrypoints de processo são relativos e não aceitam traversal;
- atualizar permissões exige nova decisão do usuário;
- falhas de persistência não publicam o novo estado em memória;
- o estado v2 é descartado com segurança ao migrar para o estado v3;
- o adapter Codex passa a verificar permissões tipadas além de estar ativo.
- a integridade do pacote depende de um descritor obtido por canal confiável;
  o manifesto interno, sozinho, não autentica seu próprio conteúdo.
