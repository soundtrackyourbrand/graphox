import { gql } from 'graphql-tag';
import { USER_FRAGMENT } from './user_fragment';

const GET_USER_WITH_FRAGMENT = gql`
  query GetUserWithFragment {
    user {
      ...UserFragment
    }
  }
  ${USER_FRAGMENT}
`;
