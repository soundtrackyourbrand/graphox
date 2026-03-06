const f1 = gql`
  fragment PublicFragment on User @public {
    id
    name
    profile {
      ...PrivateFragment
    }
  }
`;

const f2 = gql`
  fragment PrivateFragment on Profile {
    bio
  }
`;
