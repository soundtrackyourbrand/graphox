/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type FragmentType<TFragment> = TFragment extends { __fragment: infer T }
  ? T
  : never;

export function getFragmentData<TFragment, TData>(
  _fragment: TFragment,
  data: TData
): FragmentType<TFragment> {
  return data as FragmentType<TFragment>;
}
