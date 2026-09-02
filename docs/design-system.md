# Design system do workspace Lyrnova

## Linguagem

O workspace combina a densidade de um coding agent moderno com a linguagem
Lyra. Lyra Welcome orienta onboarding e empty states; Lyra Installer orienta
rail, hierarquia e progressão. A composição de desenvolvimento é própria.

## Layout

- activity bar esquerda: Explorer, Busca, Git, Conversas e Conta;
- sidebar contextual: árvore de arquivos, source control ou threads;
- header: projeto, path, branch, ambiente e ações;
- centro: editor com abas, sempre priorizado;
- painel direito: chat do agente; arquivos/alterações o substituem sob demanda;
- dock inferior: terminal/output;
- command palette: navegação e ações globais.
- titlebar própria: minimizar, maximizar/restaurar e fechar no padrão Draco.

Em 1440×900 sidebar, editor, chat e dock podem coexistir. Abaixo de 900 px,
sidebar e painel direito tornam-se overlays controlados. Em 800×600 a prioridade
é o editor, com chat, arquivos, alterações e terminal recolhíveis.

## Tokens

Tokens ficam em `ui/styles.css`:

- navy profundo para fundo e superfícies;
- violeta Lyrnova como acento;
- ciano para contexto/conexão;
- verde, amarelo e vermelho para estados com rótulos redundantes;
- bordas de baixo contraste e raios de 7–14 px;
- monospace do sistema para diff e terminal;
- dourado para foco visível.

## Aparência adaptável

A preferência local pode seguir o sistema ou fixar os temas Lyrnova escuro,
claro e alto contraste. As paletas usam os mesmos tokens semânticos e incluem
temas correspondentes do Monaco. A fonte global varia de 13 a 20 px sem alterar
a preferência independente do editor. Densidade compacta reduz alturas e
espaçamentos; redução de movimento desativa animações, transições e rolagem suave.

## Regras

- código, diff e texto têm prioridade sobre decoração;
- vermelho/verde nunca são os únicos indicadores de diff;
- approval mostra ação, cwd, autoridade e rede;
- inspector e terminal não cobrem permanentemente editor ou composer;
- abrir um arquivo no inspector mantém o editor central e restaura o chat à
  direita;
- estado não salvo é textual e visual; `Ctrl+S` confirma persistência real ou
  informa explicitamente erro/conflito;
- abas seguem convenções de IDE: `×`, `Ctrl+W`, indicador dirty e confirmação
  antes de descartar;
- input do usuário entra no DOM por `textContent`, não `innerHTML`;
- nenhuma dependência visual remota;
- controles de janela usam allowlist Tauri explícita e fechamento confirma
  descarte quando houver rascunho alterado;
- movimento reduzido desativa transições não essenciais.

## Protótipo atual

O protótipo é navegável e oferece:

- toggle de sidebar, inspector e terminal;
- tabs de arquivos/alterações;
- approval real de comando, arquivo ou rede com decisão de uso único, sessão,
  negação ou cancelamento;
- command palette;
- nova thread;
- composer com resposta simulada;
- Monaco Editor com arquivos reais, syntax highlighting por extensão,
  autocomplete, abas, minimap, gutter, cursor e `Ctrl+S` protegido por revisão;
- Explorer com ícones por tipo e área Git inspirada na organização do VS Code;
- foco rápido do chat/editor por botões ou `Ctrl+1`/`Ctrl+2`;
- breakpoints e live regions.

No shell Tauri de debug, explorer e editor leem e salvam arquivos UTF-8
existentes dentro da raiz autorizada. No navegador estático, fixtures em memória
mantêm o protótipo navegável. O agente e o terminal ainda não executam comandos
ou alterações reais.
