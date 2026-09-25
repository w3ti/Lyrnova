# Diagnósticos Rust locais

Instale Bubblewrap e disponibilize um `rust-analyzer` standalone ELF em uma pasta
absoluta do PATH usado para iniciar o Lyrnova. O IDE não baixa executáveis. Um
launcher do rustup não é aceito nesta primeira integração; use o binário real da
toolchain ou a [distribuição standalone oficial](https://rust-analyzer.github.io/book/installation.html).

1. Abra um projeto e um arquivo `.rs`.
2. No painel **Problemas**, escolha **Ativar análise Rust…**.
3. Revise as permissões de leitura do workspace e execução de processos.
4. Edite o arquivo. Os diagnósticos aparecem no editor e na lista; clicar em um
   resultado posiciona o cursor. Não é preciso salvar o rascunho para analisá-lo.

Também é possível ativar/desativar pelo cartão Rust em Configurações. Desativar
encerra o servidor e remove os marcadores. O botão **Reiniciar análise** recupera
falhas ou refaz a descoberta após mudanças na configuração do projeto. Falta de
servidor/sandbox aparece como erro no próprio painel; a edição continua disponível.

Esta etapa analisa arquivos abertos com metadados locais: não carrega dependências
externas/biblioteca padrão nem executa os checks usuais de Cargo. Build scripts e
proc macros ficam desativados na configuração; overrides existentes nas pastas de
código são ocultados dentro do sandbox. O processo possui leitura do workspace e
não tem rede nem escrita nele. Configurações de toolchain, fontes externas e
suporte completo de linguagem serão entregas seguintes.

Limites: 32 arquivos, 512 KiB por arquivo e 8 MiB de texto total. Diagnósticos
truncados ou arquivos acima do limite são indicados na interface. A recuperação de
rascunhos continua independente da análise de linguagem.

Contrato técnico e limitações: [ADR-0021](../architecture/ADR-0021-rust-lsp-diagnostics.md).
