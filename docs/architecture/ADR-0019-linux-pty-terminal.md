# ADR-0019: terminal interativo PTY no Linux

- Data: 2026-09-25
- Estado: implementado localmente
- Relacionado: #18, ADR-0016, jornada nativa do MVP

## Decisão

Substituir stdin/stdout/stderr por pipes por um PTY Linux, com Bash interativo
(`--noprofile --norc -i`) e xterm.js 6.0.0 / addon-fit 0.11.0 empacotados localmente.
O terminal usa o usuário do aplicativo e o diretório do projeto autorizado.
O kernel precisa oferecer pidfds (Linux 5.3 ou posterior).
Não executa configuração de shell do workspace nem recebe executável/cwd pelo IPC.
O terminal local continua distinto de Tasks isoladas e de ferramentas de agentes.

O backend abre master/slave com CLOEXEC, cria uma sessão com `setsid` e atribui
o slave como terminal controlador. O master usa I/O não bloqueante. Ctrl+C,
Ctrl+D, modo raw, programas de tela inteira, eco e job control usam a disciplina
do terminal do sistema. stdout e stderr são combinados, como em um terminal real.
O xterm interpreta ANSI/UTF-8 e mantém até 5.000 linhas de scrollback em memória.

## Protocolo e limites

- `terminal_start(cols, rows)` retorna um identificador opaco da sessão.
- `terminal_write(sessionId, input)` recebe bytes, até 16 KiB por chamada e
  até 128 KiB pendentes no backend. A fila da UI também é limitada a 128 KiB.
- `terminal_resize(sessionId, cols, rows)` permite 2–500 colunas e 1–500 linhas;
  o tamanho calculado pelo addon-fit é aplicado com `TIOCSWINSZ`.
- `terminal_ack(sessionId, sequence)` libera saída já processada pelo xterm;
  sequência zero confirma que listener e identificação estão prontos.
- `terminal_stop(sessionId)` encerra somente a sessão indicada. Identificadores
  antigos falham também em write, resize e ack.
- Eventos `terminal-event` identificam a sessão e carregam bytes/sequência ou o
  código/sinal de saída. No máximo oito chunks de até 8 KiB ficam sem confirmação.
  A leitura do master pausa quando essa janela enche; entrada e cancelamento
  continuam sendo processados. ACKs além da última sequência enviada são ignorados.

Start/stop usam o pool bloqueante do Tauri. Um worker por sessão cuida de leitura,
escrita e resize; a thread da UI não espera o programa do terminal consumir dados.
Trocar de projeto é serializado com startup do PTY. Na UI, tokens e uma barreira
de reset impedem respostas/bytes atrasados de reaparecerem em uma nova sessão.

## Encerramento

Bash interativo cria grupos separados para jobs. Encerrar apenas seu PID ou
grupo não basta. No Linux, o worker identifica os grupos da própria sessão em
`/proc`, interrompe sua execução e os encerra antes de recolher o shell.
`waitid(WNOWAIT)` mantém reservado o PID do líder até a limpeza, inclusive após
EOF/exit; jobs desassociados com `disown` mas na mesma sessão são encerrados.
Os membros descobertos são sinalizados por pidfds; reutilização de um PID numérico
durante a varredura não pode direcionar o sinal para um processo alheio.

Reiniciar, trocar de workspace e fechar a janela principal descartam a sessão.
Ocultar o painel preserva a execução. A espera de descarte é limitada a um segundo;
se um processo estiver em espera ininterruptível do kernel, o worker continua
responsável pela limpeza/reaping em background. Não se promete contenção de código
que deliberadamente crie outra sessão (`setsid`), nem cleanup após SIGKILL/crash do
host. Contenção forte de processos exige cgroups ou mecanismo equivalente.

## Fronteiras e escopo

A saída do PTY nunca vira HTML, comando de navegação ou chamada do backend.
OSC 52 (clipboard) e OSC 8 (links) são descartados; não há addons de clipboard,
links, imagens ou recursos remotos. Operações de janela por sequências de terminal
ficam desativadas. As permissões Tauri não foram ampliadas nesta entrega.
O terminal não é sandbox: comandos explícitos do usuário possuem sua autoridade
local. Agentes/plugins continuam sujeitos aos brokers e grants existentes.

Há uma sessão efêmera para o workspace atual; reiniciar substitui essa sessão,
e trocar de projeto a encerra. Não há múltiplas abas de terminal, restauração,
histórico persistido, perfis configuráveis ou execução automática de comandos.
Outras plataformas retornam `unsupported`; esta decisão não afirma portabilidade
para macOS/Windows nem homologação completa de IME/acessibilidade.

## Validação

Testes Rust abrem PTYs reais e verificam TTY em stdin/stdout/stderr, tamanho,
ANSI/Unicode, Ctrl+C, EOF, cleanup de jobs foreground/background/disown, limites,
backpressure e recusa de identificadores obsoletos. A jornada nativa usa o handler
de colagem e eventos de tecla do xterm, confere cores renderizadas, resize no
shell, Ctrl+C/Ctrl+D, reinício e fechamento; editor/Git/Tasks seguem no mesmo gate.
O método de entrada DOM e seus limites de homologação continuam documentados no
[guia da suíte](../../tests/e2e/README.md).

Referências: [disciplina do PTY Linux](https://man7.org/linux/man-pages/man7/pty.7.html),
[segurança do xterm.js](https://xtermjs.org/docs/guides/security/) e
[controle de fluxo do xterm.js](https://xtermjs.org/docs/guides/flowcontrol/).
