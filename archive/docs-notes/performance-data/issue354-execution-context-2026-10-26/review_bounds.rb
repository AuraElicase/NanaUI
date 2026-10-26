require 'json'
require 'digest'
root=ARGV.fetch(0)
result=[]
Dir.glob(File.join(root,'g*-*-*.json')).sort.each do |file|
 next if file.end_with?('-receipt.json','.launch.json','.complete.json')
 report=JSON.parse(File.read(file)); numer,denom=report.fetch('mach_timebase'); guard=8*Rational(numer,denom); counts=Hash.new(0); sequence=[]; births=[]
 rows=report.fetch('timing_samples_detail').fetch('series').fetch('phases')[0].fetch('samples').select{|s|s.fetch('included_in_measurement')}
 cpu_lo=0;cpu_hi=0;p_lo=0;p_hi=0
 rows.each do |s|
  before,after=s.fetch('process_counter_endpoints');raise 'query' unless before.fetch('error')==0 && after.fetch('error')==0
  a,b=before.fetch('values'),after.fetch('values');births<<a.fetch('process_start_abstime');raise 'birth' unless births[-1]==b.fetch('process_start_abstime')
  c=s.fetch('thread_cpu_ns');total=Rational((b.fetch('user_cpu_ticks')-a.fetch('user_cpu_ticks'))+(b.fetch('system_cpu_ticks')-a.fetch('system_cpu_ticks')))*numer/denom
  ptime=Rational((b.fetch('performance_user_cpu_ticks')-a.fetch('performance_user_cpu_ticks'))+(b.fetch('performance_system_cpu_ticks')-a.fetch('performance_system_cpu_ticks')))*numer/denom
  raise 'enclosure' unless total+guard>=c && ptime>=0 && ptime<=total
  # Algebraically different test of the Python lower-bound inequalities:
  # (C - other - 2G)/(C + G) >= .95 iff C >= 20*other + 59G.
  cpu_lo += [0,c-guard].max;cpu_hi += c+guard
  p_lo += [0,c-total+ptime-2*guard].max;p_hi += [c+guard,ptime+guard].min
  p95=c>=20*(total-ptime)+59*guard
  e95=c>=20*ptime+59*guard
  raise 'overlap' if p95 && e95
  kind=p95 ? 'P95-bound' : e95 ? 'E95-bound' : 'ambiguous';counts[kind]+=1;sequence<<kind
 end
 raise 'incarnation' unless births.uniq.length==1
 lp=p_lo/cpu_hi;up=[1,p_hi/cpu_lo].min
 result<<{p_time_fraction_bounds:{lower:{numerator:lp.numerator,denominator:lp.denominator},upper:{numerator:up.numerator,denominator:up.denominator}},name:File.basename(file,'.json'),frames:rows.length,groups:counts,class_sequence_sha256:Digest::SHA256.hexdigest(sequence.join(',')),raw_sha256:Digest::SHA256.file(file).hexdigest}
end
puts JSON.pretty_generate(result)
