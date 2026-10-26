#include <libproc.h>
#include <sys/resource.h>
#include <stdio.h>
#include <stdint.h>
#include <unistd.h>
#include <time.h>
#include <mach/mach_time.h>
#include <stddef.h>
_Static_assert(sizeof(struct rusage_info_v6) == 464, "public V6 ABI size");
_Static_assert(offsetof(struct rusage_info_v6, ri_instructions) == 248, "instructions offset");
_Static_assert(offsetof(struct rusage_info_v6, ri_cycles) == 256, "cycles offset");
_Static_assert(offsetof(struct rusage_info_v6, ri_user_ptime) == 304, "performance time offset");
_Static_assert(offsetof(struct rusage_info_v6, ri_pinstructions) == 320, "performance instructions offset");
static uint64_t ns(struct timespec x) { return (uint64_t)x.tv_sec*1000000000+x.tv_nsec; }
int main(void) {
 struct rusage_info_v6 a={0},b={0};struct timespec ca,cb;mach_timebase_info_data_t base;mach_timebase_info(&base);
 int first=proc_pid_rusage(getpid(),RUSAGE_INFO_V6,(rusage_info_t *)&a);clock_gettime(CLOCK_PROCESS_CPUTIME_ID,&ca);
 uint64_t end=mach_absolute_time()+((uint64_t)300000000*base.denom/base.numer),sum=0;
 while(mach_absolute_time()<end)for(int i=0;i<10000;i++)sum=sum*1664525+1013904223;
 clock_gettime(CLOCK_PROCESS_CPUTIME_ID,&cb);int last=proc_pid_rusage(getpid(),RUSAGE_INFO_V6,(rusage_info_t *)&b);
 uint64_t total=b.ri_user_time+b.ri_system_time-a.ri_user_time-a.ri_system_time;
 printf("{\"errors\":[%d,%d],\"size\":%zu,\"numer\":%u,\"denom\":%u,\"raw_cpu_delta\":%llu,\"converted_cpu_ns\":%.0f,\"process_clock_ns\":%llu,\"instructions\":%llu,\"cycles\":%llu,\"pinstructions\":%llu,\"pcycles\":%llu,\"ptime_raw\":%llu,\"checksum\":%llu}\n",first,last,sizeof(a),base.numer,base.denom,total,(double)total*base.numer/base.denom,ns(cb)-ns(ca),b.ri_instructions-a.ri_instructions,b.ri_cycles-a.ri_cycles,b.ri_pinstructions-a.ri_pinstructions,b.ri_pcycles-a.ri_pcycles,b.ri_user_ptime+b.ri_system_ptime-a.ri_user_ptime-a.ri_system_ptime,sum);
 return first||last;
}
