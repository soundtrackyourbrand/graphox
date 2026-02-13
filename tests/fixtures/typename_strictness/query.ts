const q = gql`
  fragment UserIdOnly on User {
    id
  }

  query TestQuery {
    user {
      ...UserIdOnly
    }
  }
`;
