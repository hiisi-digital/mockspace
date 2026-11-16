# Mutations of the cross-platform disassembly, and the tests each turns red

Each fix of `work/disasm-cross-platform` taken back out of the code one at a
time, on x86_64 Linux with `llvm-objdump` 18.1.3 and GNU `objdump` 2.42. Every
`<name>.patch` is the mutation as a diff from the branch; every `<name>.out`
opens with the command that replays it and holds the test lines that run
printed. A row below lists the tests that failed under its mutation, read off
its `.out`.

| mutation | what it takes back out | failing tests |
|---|---|---|
| `m01-gnu-header` | dropping the file-format header line | `disasm::tests::a_gnu_file_header_does_not_make_two_libraries_differ`, `disasm::tests::every_tools_header_and_label_lines_are_dropped`, `disasm::tests::output_with_no_instructions_is_no_evidence`, `disasm::tests_recorded::identical_work_in_two_libraries_is_one_duplicate_on_every_format_and_tool`, `disasm::tests_live::identical_work_in_separate_crates_is_flagged_duplicate` |
| `m02-gnu-symbol-flag` | GNU's own symbol flag | `disasm::tests_live::bench_entry_is_found_under_this_platforms_symbol_spelling`, `disasm::tests_live::every_tool_here_finds_bench_entry_in_a_linked_library_of_every_format_it_reads`, `disasm::tests_live::different_work_is_not_flagged_duplicate` |
| `m03-host-naming` | naming a variant by its file's suffix | `disasm::tests::a_variant_is_named_by_its_own_name_not_by_its_build_directory` |
| `m04-rip-fold` | the RIP-relative fold | `disasm::tests::a_rip_relative_displacement_is_layout_and_folds`, `disasm::tests_recorded::the_same_entry_at_other_addresses_normalises_to_the_same_text` |
| `m05-bare-hex-target` | folding a target written without 0x | `disasm::tests::an_annotated_target_folds_whether_or_not_it_is_written_with_0x`, `disasm::tests_recorded::the_same_entry_at_other_addresses_normalises_to_the_same_text`, `disasm::tests_live::different_work_is_not_flagged_duplicate` |
| `m06-semicolon-comment` | the ; comment rule | `disasm::tests::every_tools_comment_comes_off_and_no_immediate_does` |
| `m07-macho-spelling` | Mach-O's underscore | `disasm::tests_live::every_tool_here_finds_bench_entry_in_a_linked_library_of_every_format_it_reads`, `disasm::tests_tool::every_format_is_read_off_its_own_leading_bytes_and_spells_a_symbol_its_way` |
| `m08-empty-is-evidence` | an empty disassembly is no evidence | `disasm::tests::output_with_no_instructions_is_no_evidence` |
| `m09-adrp-page` | folding an adrp page in every form | `disasm::tests::an_adrp_page_folds_in_every_form_and_its_companion_add_with_it` |
| `m10-adrp-companion` | folding the add after an adrp | `disasm::tests::an_adrp_page_folds_in_every_form_and_its_companion_add_with_it`, `disasm::tests_recorded::identical_work_in_two_libraries_is_one_duplicate_on_every_format_and_tool`, `disasm::tests_recorded::the_same_entry_at_other_addresses_normalises_to_the_same_text` |
| `m11-label-contains` | the exact label match | `disasm::tests_recorded::bench_entry_is_found_under_the_formats_own_spelling_and_no_other`, `disasm::tests_tool::a_symbols_block_runs_from_its_exact_label_to_the_next_label` |
| `m12-dumpbin-no-fold` | dumpbin's address and name folds | `disasm::tests::dumpbin_full_width_addresses_and_mangled_names_fold` |
| `m13-dumpbin-folds-immediates` | keeping long dumpbin immediates | `disasm::tests::dumpbin_full_width_addresses_and_mangled_names_fold` |
| `m14-gnu-pe-exports` | GNU's export lookup in a DLL | `disasm::tests_live::every_tool_here_finds_bench_entry_in_a_linked_library_of_every_format_it_reads` |
| `m15-pe-i386-spelling` | the bare spelling first for a 32-bit PE image | `disasm::tests_tool::every_format_is_read_off_its_own_leading_bytes_and_spells_a_symbol_its_way` |
| `m16-dylib-check-name` | dylib_check naming its library as the host does | `dylib_check::tests::a_module_library_is_named_as_this_host_names_one` |
| `m17-build-for-empty-prefix` | build_for reading the file's own prefix (as on Windows) | `bench_variants::tests::an_artifact_path_with_its_platform_name_finds_the_same_crate`, `bench_variants::tests::an_artifact_path_named_for_any_target_finds_the_same_crate_on_every_host` |
| `m18-layout-placement` | the layout placing a disasm artifact | `tests::a_disassembly_artifact_is_in_the_layout_and_no_run_reader_takes_it_for_a_run` |
| `m19-layout-dotted-names` | the arm's own directory under a disasm run | `tests::a_disassembly_artifact_is_in_the_layout_and_no_run_reader_takes_it_for_a_run` |
| `m20-artifact-overwrites` | exclusive creation of an artifact | `disasm::artifact::tests::a_file_already_there_is_never_replaced`, `disasm::artifact::tests::a_second_artifact_takes_the_next_run_and_says_which` |
| `m21-artifact-header-run` | the header naming the run it sits in | `disasm::artifact::tests::the_header_names_the_run_directory_the_artifact_sits_in` |
| `m22-help-without-disasm` | disasm in mock help bench | `entry::clean_tests::mock_help_bench_names_every_bench_subcommand` |
| `m23-recorded-pair-made-identical` | a recorded pair differing in its instructions (fixture edit) | `disasm::tests_recorded::every_recorded_pair_differs_in_its_instruction_lines_before_normalisation` |
