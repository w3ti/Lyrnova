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
- ícones da barra de atividades esquerda em 1,5 rem (24 px por padrão), com
  botões de 48 px, ou 42 px na densidade compacta;
- foco rápido do chat/editor por botões ou `Ctrl+1`/`Ctrl+2`;
- breakpoints e live regions.

No shell Tauri de debug, explorer e editor leem e salvam arquivos UTF-8
existentes dentro da raiz autorizada. No navegador estático, fixtures em memória
mantêm o protótipo navegável. No aplicativo desktop, o terminal executa comandos
reais em um PTY Linux com `/bin/bash -i` e xterm.js: ANSI, Unicode, resize,
Ctrl+C e Ctrl+D. Ctrl+Shift+C copia a seleção do terminal; Ctrl+Shift+V cola o
clipboard. A fonte do terminal continua configurável. O botão `+` reinicia
a sessão atual; ocultar o painel preserva o shell. Tasks de plugins passam pelo broker de processos e por revisão
explícita. O agente é opcional e só fica disponível com um provider autorizado.

A barra do editor diferencia salvamento do arquivo e backup da sessão. Rascunhos
recuperados com divergência no disco mostram um aviso junto ao documento, com ações
para salvar uma cópia ou descartar explicitamente e reler. A recuperação preserva
ordem das abas, arquivo ativo, cursor e rolagem; não reexecuta comandos do terminal.

Com a análise Rust ativada, F12 abre a definição em uma aba do projeto. Hover
mostra tipos/documentação como texto inerte e também pode ser aberto por
F1 → **Show or Focus Hover**. Ctrl+K abre a paleta do Lyrnova inclusive no editor;
os demais atalhos do Monaco, como Ctrl+Shift+K, continuam disponíveis.

A paleta usa um diálogo modal: Tab/Shift+Tab ficam entre seus controles, o foco
é visível e Escape retorna ao elemento de origem mesmo após Ctrl+K repetido.
Executar um comando fecha a paleta antes de focar o próximo painel ou diálogo.
Atalhos globais respeitam os outros modais e a composição de texto. Ocultar o
terminal com foco nele devolve o foco ao editor sem encerrar o shell.
Diagnósticos aparecem como marcadores no Monaco e itens navegáveis
no painel Problemas; editar remove os resultados obsoletos imediatamente.

**Ambiente Rust** revisa a toolchain, a disponibilidade de rust-src e os diretórios
do registry; a opção de compartilhar cache começa desmarcada. O painel apresenta
as limitações efetivas e mensagens do servidor. Definições externas abrem em um
visualizador somente leitura, separado das abas e rascunhos do projeto.

Ações Rust usam atalhos familiares: Ctrl+Espaço para sugestões, Shift+F12 para
referências, F2 para renomear e Shift+Alt+F para formatar. Referências aparecem em
um diálogo com botões nomeados por arquivo/linha/coluna, status anunciado e navegação
por teclado. Renomear abre arquivos afetados em abas e marca os rascunhos; desfazer
é por arquivo e salvar permanece explícito. Formatação usa o formatter da toolchain.

A barra do editor informa se a atualização automática de arquivos está ativa ou
indisponível. Mudanças externas recarregam abas limpas e atualizam Explorer/busca.
Rascunhos, exclusões e substituições incompatíveis usam o aviso de conflito já
existente, com salvar cópia ou reler explicitamente. Atualizações repetidas não
apagam mensagens de falha ao salvar. A análise Rust mostra novamente seu estado
de inicialização ao recarregar fontes externas ao editor.

### Correções rápidas Rust

Com análise Rust ativa, Ctrl+. e a lâmpada do Monaco oferecem correções rápidas
aplicáveis à seleção. As ações alteram rascunhos e participam do desfazer; não
salvam automaticamente. Ctrl+Espaço inclui auto-imports prontos para aceitação,
inserindo símbolo e `use` juntos. Resultados obsoletos são recusados, e falhas na
aplicação aparecem no status de ações da análise. Comandos do servidor e ações
que criam, removem ou renomeiam arquivos não aparecem nesta etapa.
