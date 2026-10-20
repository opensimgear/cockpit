/** Compare versions within a release series. Date versions predate component SemVer. */
export function compareFirmwareVersions(left:string,right:string):number|undefined {
  const semantic=/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
  const legacy=/^\d{4}\.\d+\.\d+\.\d+$/;
  if(!(semantic.test(left)&&semantic.test(right)) && !(legacy.test(left)&&legacy.test(right)))return;
  const a=left.split('.').map(Number),b=right.split('.').map(Number);
  for(let index=0;index<a.length;index++)if(a[index]!==b[index])return a[index]>b[index]?1:-1;
  return 0;
}

export function firmwareUpdateAvailable(installed?:string,available?:string|null):boolean {
  if(!installed||!available||installed===available)return false;
  const comparison=compareFirmwareVersions(installed,available);
  // A published SemVer release also replaces the legacy development version series.
  return comparison===undefined || comparison<0;
}

export function firmwareMeetsAvailable(installed?:string,available?:string|null):boolean {
  if(!installed||!available)return false;
  return installed===available || (compareFirmwareVersions(installed,available)??-1)>=0;
}
