# ADR-0003: Separar intenção de plugins e autoridade do IDE

- Status: aceito como princípio
- Data: 2026-09-01

## Decisão

Um plugin de linguagem, ferramenta ou IA pode solicitar leitura, busca, patch,
comando ou acesso externo. Somente o núcleo Lyrnova decide se a ação cabe na
autoridade atual. O frontend apresenta approvals e envia decisões; ele não
executa o efeito. Plugins de IA, quando instalados, seguem as mesmas regras.

Os modos previstos são:

- `read-only`: sem escrita nem execução mutável;
- `workspace-write`: alterações somente nos roots autorizados;
- `escalated`: ação específica fora do modo normal após approval vinculada ao
  conteúdo exibido.

Rede e paths externos são negados por padrão. Alterar comando, cwd, path ou
domínio invalida uma aprovação anterior.

## Consequências

- ferramentas recebem argumentos estruturados;
- paths são canonicalizados antes da decisão e do efeito;
- subprocessos precisam de sandbox, limites e cleanup;
- arquivos e output do repositório são dados não confiáveis;
- instruções AGENTS.md orientam o trabalho, mas não ampliam autoridade;
- cancelamento impede novos efeitos do turn.
