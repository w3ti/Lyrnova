# ADR-0008: Revisão local de plugins na fronteira Tauri

- Status: aceito
- Data: 2026-09-02

## Contexto

O instalador transacional e o catálogo dinâmico já validam e persistem pacotes,
mas expor esse fluxo à interface cria uma nova fronteira de confiança. O frontend
não deve escolher paths arbitrários para o núcleo, reaproveitar um staging antigo
ou alterar silenciosamente as permissões entre a revisão e a confirmação.

## Decisão

A seleção do `.tar.zst` acontece exclusivamente no seletor nativo aberto pelo
command Rust. O núcleo deriva o descritor adjacente `<arquivo>.json`, valida ambos
e mantém no máximo um staging pendente em memória. A interface recebe somente uma
projeção tipada para revisão e um token UUID opaco; paths do filesystem e o objeto
de staging nunca cruzam a fronteira Tauri.

A confirmação precisa apresentar o mesmo token e o conjunto exato de permissões
do manifesto. O núcleo repete essa comparação antes de consumir o staging. Um
token ausente, substituído ou já consumido falha fechado. Cancelar remove o staging
temporário, e iniciar outra seleção descarta o anterior. Staging e confirmação
são serializados por um lock global do fluxo, inclusive sob chamadas IPC
concorrentes.

Depois do rename atômico, o catálogo registra a versão e as permissões revisadas,
mas mantém o plugin desabilitado. Ativação é uma ação posterior e explícita. Dados
não confiáveis do manifesto são renderizados com nós DOM e `textContent`, nunca
como HTML.

## Consequências

- o frontend não ganha uma API genérica de leitura de paths locais;
- revisão e confirmação ficam vinculadas a uma única sessão efêmera;
- alterar ou omitir uma permissão no IPC não reduz a autoridade aprovada;
- fechar o aplicativo também descarta qualquer staging ainda pendente;
- a convenção de sidecar local não autentica o publisher; downloads curados foram
  definidos pela ADR-0010, enquanto assinatura e catálogo remoto autenticado
  continuam sendo trabalho separado;
- remoção física e execução sandboxed de pacotes externos permanecem etapas
  posteriores.
