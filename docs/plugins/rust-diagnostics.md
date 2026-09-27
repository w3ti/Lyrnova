# Análise Rust local

Instale Bubblewrap e disponibilize um `rust-analyzer` standalone ELF em uma pasta
absoluta do PATH usado para iniciar o Lyrnova. O IDE não baixa executáveis. Um
launcher do rustup não é aceito como servidor; use o binário real da toolchain ou
a [distribuição standalone oficial](https://rust-analyzer.github.io/book/installation.html).

1. Abra um projeto e um arquivo `.rs`.
2. No painel **Problemas**, escolha **Ativar análise Rust…** e revise as permissões.
3. Abra **Ambiente Rust…** para selecionar Cargo/rustc do sistema ou uma toolchain
   já instalada pelo rustup. O caminho e a presença das fontes da biblioteca padrão
   são mostrados na revisão.
4. Para analisar dependências, mantenha **Analisar dependências locais** marcado.
   Se elas estiverem no cache do Cargo, autorize explicitamente a leitura dos
   diretórios exibidos. Clique em **Aplicar e reiniciar**.
5. Edite o arquivo: os diagnósticos aparecem sem salvar o rascunho. Clique em um
   resultado para navegar. Use **F12** para ir à definição e hover ou **Ctrl+K,
   Ctrl+I** para tipos/documentação. Fontes externas abrem somente para leitura.

Com a análise ativa, os recursos de edição são:

| Ação | Atalho | Resultado |
| --- | --- | --- |
| Autocompletar símbolos | Ctrl+Espaço, `.` ou `:` | Sugestões semânticas e auto-imports junto dos snippets existentes |
| Correção rápida | Ctrl+. ou lâmpada do editor | Aplica uma correção ao rascunho, incluindo imports de símbolos não resolvidos |
| Encontrar referências | Shift+F12 ou menu de contexto | Lista navegável, incluindo a declaração |
| Renomear símbolo | F2 | Atualiza rascunhos dos arquivos afetados e abre os que estavam fechados |
| Formatar documento | Shift+Alt+F | Aplica rustfmt da toolchain selecionada ao rascunho |

Correções rápidas, auto-imports, renomeação e formatação não salvam os arquivos. Use Ctrl+Z para desfazer em cada
arquivo e Ctrl+S para salvar. O backup de sessão inclui as abas afetadas. Conflitos,
resultados obsoletos e renomeações de arquivos/bibliotecas externas são recusados.
A formatação exige `rustfmt` instalado na toolchain selecionada; não há instalação
automática. Nome inválido ou ferramenta indisponível preservam o rascunho.

Auto-import insere o símbolo e seu `use` juntos; um Ctrl+Z desfaz ambos. A lista
resolve até 32 candidatos por consulta; digite mais para refinar os resultados.
Correções que exigem comandos ou criação/remoção de arquivos não são oferecidas.
Renomeação e correções rápidas preparam até 32 abas;
o levantamento inicial das fontes limita-se a 512 arquivos Rust / 8 MiB. A lista
de referências aceita até 256 localizações. Mudanças nas fontes ou configuração Rust dentro do projeto são detectadas
automaticamente: a análise reinicia preservando rascunhos e o ambiente autorizado.
Durante a reindexação, os resultados antigos são descartados. Fontes externas de
toolchains/cache ainda exigem reinício manual.

O padrão usa Cargo/rustc da distribuição em `/usr/bin`, sem dependências externas.
A biblioteca padrão exige rust-src instalado na toolchain. Se o sistema não tiver
ferramentas, **Ambiente Rust** continua disponível para selecionar uma instalação
rustup existente. O IDE não aplica automaticamente rust-toolchain.toml ou overrides.

A análise é offline e preserva Cargo.lock. Prepare o projeto e suas dependências
fora da análise (por exemplo, com Cargo no terminal) antes de ativá-la. Cache
incompleto ou lockfile desatualizado pode impedir a resolução; o painel mostra a
mensagem do servidor. Dependências path/vendor dentro do workspace são suportadas;
Git caches e paths externos não são compartilhados nesta etapa. Mudanças na
configuração de ambiente exigem nova revisão ou reinício da análise.

A seleção dura apenas neste projeto e nesta sessão do aplicativo. Reiniciar o
servidor preserva a escolha; trocar projeto, desativar Rust ou fechar o aplicativo
revoga a leitura externa. Configurações pessoais e credenciais do Cargo não são
montadas; crates privadas existentes no registry aprovado ficam acessíveis à análise.

Build scripts, proc macros e os checks de compilação usuais do Cargo continuam
fora desta análise. Erros que dependem deles podem ficar incompletos. Desativar
Rust encerra o servidor e limpa marcadores; **Reiniciar análise** recupera falhas.
A edição e a recuperação dos rascunhos permanecem disponíveis sem o servidor.

Limites: 32 arquivos abertos, 512 KiB por arquivo, 8 MiB de texto sincronizado.
Arquivos/diagnósticos excedentes são indicados na interface. Fontes externas
maiores que 512 KiB não abrem pelo F12 nesta etapa.

Contratos: [diagnósticos](../architecture/ADR-0021-rust-lsp-diagnostics.md),
[definição/hover](../architecture/ADR-0022-rust-symbol-queries.md) e
[ambientes/dependências](../architecture/ADR-0023-rust-toolchains-and-local-dependencies.md) e
[ações de edição](../architecture/ADR-0024-rust-editing-actions.md) e
[alterações externas](../architecture/ADR-0025-workspace-external-changes.md) e
[correções rápidas/auto-imports](../architecture/ADR-0026-rust-quick-fixes-and-auto-imports.md).
