.section .rodata.tiny_guest,"a",@progbits
.code64
.global tiny_guest_start
.global tiny_guest_end

tiny_guest_start:
    cli
    cld
    lea rsi, [rip + banner]
    call puts

shell:
    lea rsi, [rip + prompt]
    call puts
    call read_line

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_help]
    call string_equal
    test al, al
    jnz do_help

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_info]
    call string_equal
    test al, al
    jnz do_info

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_clear]
    call string_equal
    test al, al
    jnz do_clear

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_halt]
    call string_equal
    test al, al
    jnz do_halt

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_ticks]
    call string_equal
    test al, al
    jnz do_ticks

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_ls]
    call string_equal
    test al, al
    jnz do_ls

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_pwd]
    call string_equal
    test al, al
    jnz do_pwd

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_cat]
    call string_starts_with
    test al, al
    jnz do_cat

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_echo]
    call string_starts_with
    test al, al
    jnz do_echo

    cmp byte ptr [rip + input_buffer], 0
    je shell
    lea rsi, [rip + unknown]
    call puts
    jmp shell

do_help:
    lea rsi, [rip + help_text]
    call puts
    jmp shell

do_info:
    xor eax, eax
    cpuid
    lea rsi, [rip + info_prefix]
    call puts
    call print_hex64
    mov al, 10
    call putchar
    jmp shell

do_clear:
    mov al, 0x0c
    call putchar
    jmp shell

do_halt:
    lea rsi, [rip + halt_text]
    call puts
    hlt
    jmp do_halt

do_ticks:
    rdtsc
    shl rdx, 32
    or rax, rdx
    lea rsi, [rip + ticks_prefix]
    call puts
    call print_hex64
    mov al, 10
    call putchar
    jmp shell

do_echo:
    lea rsi, [rip + input_buffer + 5]
    call puts
    mov al, 10
    call putchar
    jmp shell

do_ls:
    lea rsi, [rip + fs_listing]
    call puts
    jmp shell

do_pwd:
    lea rsi, [rip + fs_path]
    call puts
    jmp shell

do_cat:
    lea rsi, [rip + input_buffer + 4]
    lea rdi, [rip + file_readme]
    call string_equal
    test al, al
    jnz cat_readme
    lea rsi, [rip + input_buffer + 4]
    lea rdi, [rip + file_about]
    call string_equal
    test al, al
    jnz cat_about
    lea rsi, [rip + cat_missing]
    call puts
    jmp shell
cat_readme:
    lea rsi, [rip + readme_text]
    call puts
    jmp shell
cat_about:
    lea rsi, [rip + about_text]
    call puts
    jmp shell

read_line:
    lea rdi, [rip + input_buffer]
    xor ecx, ecx
.read_key:
    call getchar
    cmp al, 10
    je .done
    cmp al, 8
    je .backspace
    cmp ecx, 62
    jae .read_key
    stosb
    inc ecx
    call putchar
    jmp .read_key
.backspace:
    test ecx, ecx
    jz .read_key
    dec rdi
    dec ecx
    call putchar
    jmp .read_key
.done:
    mov byte ptr [rdi], 0
    call putchar
    ret

getchar:
    push rdx
    mov dx, 0x3f8
    in al, dx
    pop rdx
    ret

putchar:
    push rdx
    mov dx, 0x3f8
    out dx, al
    pop rdx
    ret

puts:
    push rax
    push rdx
    mov dx, 0x3f8
.next:
    lodsb
    test al, al
    jz .end
    out dx, al
    jmp .next
.end:
    pop rdx
    pop rax
    ret

string_equal:
.compare:
    mov al, byte ptr [rsi]
    cmp al, byte ptr [rdi]
    jne .different
    test al, al
    je .equal
    inc rsi
    inc rdi
    jmp .compare
.equal:
    mov al, 1
    ret
.different:
    xor eax, eax
    ret

string_starts_with:
starts_compare:
    mov al, byte ptr [rdi]
    test al, al
    je starts_equal
    cmp al, byte ptr [rsi]
    jne starts_different
    inc rsi
    inc rdi
    jmp starts_compare
starts_equal:
    mov al, 1
    ret
starts_different:
    xor eax, eax
    ret

print_hex64:
    push rax
    push rbx
    push rcx
    push rdx
    mov rbx, rax
    mov ecx, 16
.digit:
    rol rbx, 4
    mov eax, ebx
    and eax, 0x0f
    lea rdx, [rip + hex_digits]
    mov al, byte ptr [rdx + rax]
    call putchar
    loop .digit
    pop rdx
    pop rcx
    pop rbx
    pop rax
    ret

banner:
    .asciz "\nTiny64 guest OS\nRunning inside AMD-V/SVM\nType 'help' for commands.\n\n"
prompt:
    .asciz "tiny> "
help_text:
    .asciz "help         - list commands\ninfo         - show virtualized CPUID leaf\nticks        - show virtual CPU timestamp\necho <text>  - print text\nls           - list virtual files\npwd          - show virtual path\ncat <file>   - read a virtual file\nclear        - clear display\nhalt         - stop the guest\n"
info_prefix:
    .asciz "Tiny64 is active; CPUID leaf 0 EAX = 0x"
ticks_prefix:
    .asciz "virtual TSC = 0x"
unknown:
    .asciz "unknown command\n"
halt_text:
    .asciz "guest halted\n"
fs_listing:
    .asciz "README.TXT  ABOUT.TXT\n"
fs_path:
    .asciz "/\n"
cat_missing:
    .asciz "cat: file not found\n"
readme_text:
    .asciz "Tiny64 read-only virtual filesystem.\nTry: cat ABOUT.TXT\n"
about_text:
    .asciz "Tiny64 is a demo VM registered with Ferrovisor.\n"
cmd_help:  .asciz "help"
cmd_info:  .asciz "info"
cmd_clear: .asciz "clear"
cmd_halt:  .asciz "halt"
cmd_ticks: .asciz "ticks"
cmd_echo:  .asciz "echo "
cmd_ls:    .asciz "ls"
cmd_pwd:   .asciz "pwd"
cmd_cat:   .asciz "cat "
file_readme: .asciz "README.TXT"
file_about:  .asciz "ABOUT.TXT"
hex_digits:
    .ascii "0123456789abcdef"

.balign 16
input_buffer:
    .zero 64
tiny_guest_end:
