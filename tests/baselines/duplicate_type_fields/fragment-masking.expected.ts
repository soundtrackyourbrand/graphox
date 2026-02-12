/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type FragmentType<TFragment> = TFragment extends { ' $fragmentRefs'?: { [key: string]: any } }
  ? TFragment
  : TFragment extends { ' $fragmentName'?: string }
  ? TFragment
  : TFragment extends { __fragment: infer T }
  ? T
  : never;

export function getFragmentData<TFragment>(
  _fragment: TFragment,
  data: FragmentType<TFragment>
): FragmentType<TFragment>;
export function getFragmentData<TFragment>(
  _fragment: TFragment,
  data: FragmentType<TFragment> | null | undefined
): FragmentType<TFragment> | null | undefined;
export function getFragmentData<TFragment>(
  _fragment: TFragment,
  data: ReadonlyArray<FragmentType<TFragment>>
): ReadonlyArray<FragmentType<TFragment>>;
export function getFragmentData<TFragment>(
  _fragment: TFragment,
  data: ReadonlyArray<FragmentType<TFragment>> | null | undefined
): ReadonlyArray<FragmentType<TFragment>> | null | undefined;
export function getFragmentData(_fragment: any, data: any): any {
  return data;
}
