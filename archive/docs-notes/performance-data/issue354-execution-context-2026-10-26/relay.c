#include <errno.h>
#include <fcntl.h>
#include <mach/mach.h>
#include <mach/task_policy.h>
#include <pthread.h>
#include <signal.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include "relay-settings.h"
static volatile sig_atomic_t interrupted;
static void cancel(int signo) { interrupted = signo; }
static double now(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) exit(70);
    return (double)t.tv_sec + t.tv_nsec / 1e9;
}
static void string(FILE *f, const char *s) {
    fputc('"', f);
    for (const unsigned char *p=(const unsigned char *)s; *p; ++p) {
        if (*p=='"' || *p=='\\') { fputc('\\',f); fputc(*p,f); }
        else if (*p<32) fprintf(f,"\\u%04x",(unsigned)*p);
        else fputc(*p,f);
    }
    fputc('"', f);
}
static FILE *receipt(const char *output, const char *suffix) {
    size_t n=strlen(output)+strlen(suffix)+1;
    char *name=malloc(n);
    if (!name) exit(70);
    snprintf(name,n,"%s%s",output,suffix);
    int fd=open(name,O_WRONLY|O_CREAT|O_EXCL,0600);
    free(name);
    if (fd<0) { perror("receipt open"); exit(73); }
    FILE *f=fdopen(fd,"w");
    if (!f) { close(fd); exit(74); }
    return f;
}
int main(int argc, char **argv) {
    const char *output=NULL;
    char **args=calloc((size_t)argc+1,sizeof(char *));
    if (!args) return 70;
    args[0]=(char *)target;
    int used=1, stripped=0;
    for (int i=1;i<argc;++i) {
        if (!strncmp(argv[i],"-psn_",5)) { ++stripped; continue; }
        args[used++]=argv[i];
        if (!strcmp(argv[i],"--output") && i+1<argc) output=argv[i+1];
    }
    if (!output || output[0]!='/') return 64;
    if (chdir(working)) return 72;
    struct sigaction action={0};
    action.sa_handler=cancel; sigemptyset(&action.sa_mask);
    if (sigaction(SIGTERM,&action,NULL) || sigaction(SIGINT,&action,NULL) || sigaction(SIGALRM,&action,NULL)) return 71;
    task_category_policy_data_t category={0};
    mach_msg_type_number_t count=TASK_CATEGORY_POLICY_COUNT;
    boolean_t defaults=FALSE;
    kern_return_t kr=task_policy_get(mach_task_self(),TASK_CATEGORY_POLICY,(task_policy_t)&category,&count,&defaults);
    uint64_t tid=0;
    int tid_error=pthread_threadid_np(NULL,&tid);
    pid_t child=0;
    double start=now();
    int spawn_error=posix_spawn(&child,target,NULL,NULL,args,controlled_env);
    FILE *f=receipt(output,".launch.json");
    fprintf(f,"{\"relay_pid\":%d,\"relay_ppid\":%d,\"target_pid\":%d,\"spawn_error\":%d,\"relay_thread_id\":",getpid(),getppid(),child,spawn_error);
    if (tid_error) fputs("null",f); else fprintf(f,"%llu",(unsigned long long)tid);
    fprintf(f,",\"task_policy_get_kr\":%d,\"task_role_default\":%s,\"relay_role_before_spawn\":",kr,defaults?"true":"false");
    if (kr==KERN_SUCCESS && count==TASK_CATEGORY_POLICY_COUNT && !defaults) fprintf(f,"%d",category.role); else fputs("null",f);
    fprintf(f,",\"ignored_launch_psn_args\":%d,\"cwd\":",stripped); string(f,working);
    fputs(",\"argv\":[",f);
    for (int i=0;i<used;++i) { if(i) fputc(',',f); string(f,args[i]); }
    fputs("],\"environment\":[",f);
    for (int i=0;controlled_env[i];++i) { if(i) fputc(',',f); string(f,controlled_env[i]); }
    fputs("]}\n",f);
    if (fclose(f)) return 74;
    int status=0, wait_error=0, term_sent=0, kill_sent=0;
    if (!spawn_error) {
        alarm(90);
        for (;;) {
            pid_t got=waitpid(child,&status,0);
            if (got==child) break;
            if (got<0 && errno==EINTR) {
                if (!interrupted) continue;
                // Still an unreaped child: its PID cannot have been reused.
                term_sent=kill(child,SIGTERM)==0;
                double deadline=now()+2.0;
                for (;;) {
                    got=waitpid(child,&status,WNOHANG);
                    if (got==child) break;
                    if (got<0 && errno!=EINTR) { wait_error=errno; break; }
                    if (now()>=deadline) {
                        kill_sent=kill(child,SIGKILL)==0;
                        while ((got=waitpid(child,&status,0))<0 && errno==EINTR) {}
                        if(got!=child) wait_error=errno;
                        break;
                    }
                    struct timespec pause={0,10000000}; nanosleep(&pause,NULL);
                }
                break;
            }
            wait_error=errno; break;
        }
        alarm(0);
    }
    f=receipt(output,".complete.json");
    fprintf(f,"{\"relay_pid\":%d,\"target_pid\":%d,\"spawn_error\":%d,\"wait_error\":%d,\"wait_status\":%d,\"exit_code\":",getpid(),child,spawn_error,wait_error,status);
    if (!spawn_error && !wait_error && WIFEXITED(status)) fprintf(f,"%d",WEXITSTATUS(status)); else fputs("null",f);
    fputs(",\"signal\":",f);
    if (!spawn_error && !wait_error && WIFSIGNALED(status)) fprintf(f,"%d",WTERMSIG(status)); else fputs("null",f);
    fprintf(f,",\"interrupted\":%d,\"term_sent\":%d,\"kill_sent\":%d,\"elapsed_seconds\":%.9f}\n",interrupted,term_sent,kill_sent,now()-start);
    if (fclose(f)) return 74;
    free(args);
    return !spawn_error && !wait_error && !interrupted && WIFEXITED(status) && WEXITSTATUS(status)==0 ? 0 : 1;
}
