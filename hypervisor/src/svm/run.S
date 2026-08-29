.section .text.svm_vmrun,"ax",@progbits
.global svm_vmrun
.type svm_vmrun,@function
svm_vmrun:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    sub rsp, 16
    mov [rsp], rsi
    mov [rsp + 8], rdi

    mov rax, rsi
    mov rbx, [rax + 0]
    mov rcx, [rax + 8]
    mov rdx, [rax + 16]
    mov rsi, [rax + 24]
    mov rdi, [rax + 32]
    mov rbp, [rax + 40]
    mov r8,  [rax + 48]
    mov r9,  [rax + 56]
    mov r10, [rax + 64]
    mov r11, [rax + 72]
    mov r12, [rax + 80]
    mov r13, [rax + 88]
    mov r14, [rax + 96]
    mov r15, [rax + 104]
    mov rax, [rsp + 8]
    vmrun rax

    mov rax, [rsp]
    mov [rax + 0], rbx
    mov [rax + 8], rcx
    mov [rax + 16], rdx
    mov [rax + 24], rsi
    mov [rax + 32], rdi
    mov [rax + 40], rbp
    mov [rax + 48], r8
    mov [rax + 56], r9
    mov [rax + 64], r10
    mov [rax + 72], r11
    mov [rax + 80], r12
    mov [rax + 88], r13
    mov [rax + 96], r14
    mov [rax + 104], r15

    add rsp, 16
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret
.size svm_vmrun, .-svm_vmrun
